//! Human-readable v2 addresses: bech32m over the 43-byte raw Orchard address,
//! HRP `ovxt` (ORVEXA test network). The checksum catches typos; the HRP
//! keeps test addresses from being mistaken for any other network.

use bech32::{primitives::decode::CheckedHrpstring, Bech32m, Hrp};
use orchard::Address;

pub const HRP: &str = "ovxt";

pub fn encode(address: &Address) -> String {
    let hrp = Hrp::parse(HRP).expect("valid HRP");
    bech32::encode::<Bech32m>(hrp, &address.to_raw_address_bytes()).expect("43 bytes fit bech32m")
}

pub fn decode(text: &str) -> Option<Address> {
    let checked = CheckedHrpstring::new::<Bech32m>(text.trim()).ok()?;
    if checked.hrp().to_lowercase() != HRP {
        return None;
    }
    let bytes: [u8; 43] = checked.byte_iter().collect::<Vec<u8>>().try_into().ok()?;
    Option::from(Address::from_raw_address_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed_wallet::SeedWallet;

    #[test]
    fn round_trip_and_rejections() {
        let a = SeedWallet::generate().address();
        let text = encode(&a);
        assert!(text.starts_with("ovxt1") && text.len() < 90);
        assert_eq!(decode(&text), Some(a));
        assert_eq!(
            decode(&text.to_uppercase()),
            Some(a),
            "bech32 is case-insensitive"
        );
        let mut typo: Vec<char> = text.chars().collect();
        let i = typo.len() - 3;
        typo[i] = if typo[i] == 'q' { 'p' } else { 'q' };
        assert_eq!(
            decode(&typo.into_iter().collect::<String>()),
            None,
            "checksum catches a typo"
        );
        let other =
            bech32::encode::<Bech32m>(Hrp::parse("ovxm").unwrap(), &a.to_raw_address_bytes())
                .unwrap();
        assert_eq!(decode(&other), None, "other network prefix");
        let bech32_classic =
            bech32::encode::<bech32::Bech32>(Hrp::parse(HRP).unwrap(), &a.to_raw_address_bytes())
                .unwrap();
        assert_eq!(decode(&bech32_classic), None, "only bech32m");
    }
}
