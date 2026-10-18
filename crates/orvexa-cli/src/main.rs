use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use orvexa_core::{Genesis, MIN_FEE, SignedTransaction, Transaction, VERSION};
use orvexa_crypto::{Address, Hash32, random_bytes};
use orvexa_rpc::Client;
use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(about = "ORVEXA transparent devnet CLI — test atoms only", version)]
struct Args {
    #[arg(long, global = true, default_value = "http://127.0.0.1:4242")]
    node: String,
    /// Pin the intended network before signing or querying a node.
    #[arg(long, global = true)]
    expect_chain: Option<Hash32>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    WalletNew {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        password_stdin: bool,
    },
    Address {
        #[arg(long)]
        wallet: PathBuf,
    },
    /// Authenticate an encrypted backup locally, without contacting a node.
    WalletVerify {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        password_stdin: bool,
    },
    Genesis {
        #[arg(long)]
        authority: Address,
        #[arg(long)]
        recipient: Address,
        #[arg(long)]
        out: PathBuf,
    },
    Status,
    /// Inspect automatic peer synchronization, role and conflict quarantine.
    Network,
    Balance {
        #[arg(long)]
        address: Address,
    },
    History {
        #[arg(long)]
        address: Address,
    },
    Send {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        to: Address,
        #[arg(long)]
        amount: u64,
        #[arg(long, default_value_t = MIN_FEE)]
        fee: u64,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Locally sign the next experimental single-authority block.
    Produce {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Submit an already signed JSON transaction (contains no secret key).
    Submit {
        #[arg(long)]
        file: PathBuf,
    },
    Sync {
        #[arg(long)]
        peer: String,
    },
}
fn print(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn password(stdin: bool, confirm: bool) -> Result<Zeroizing<String>> {
    if stdin {
        let mut value = Zeroizing::new(String::new());
        std::io::stdin().take(4097).read_to_string(&mut value)?;
        ensure!(value.len() <= 4096, "password input exceeds limit");
        let length = value.trim_end_matches(['\r', '\n']).len();
        value.truncate(length);
        ensure!(
            !value.contains(['\r', '\n']),
            "password stdin must contain one line"
        );
        return Ok(value);
    }
    let value = Zeroizing::new(rpassword::prompt_password("Wallet password: ")?);
    if confirm {
        let confirmation = Zeroizing::new(rpassword::prompt_password("Repeat password: ")?);
        ensure!(*value == *confirmation, "passwords do not match");
    }
    Ok(value)
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::WalletNew {
            wallet,
            password_stdin,
        } => {
            let password = password(password_stdin, true)?;
            println!("{}", orvexa_wallet::create(&wallet, &password)?);
        }
        Command::Address { wallet } => println!("{}", orvexa_wallet::address(&wallet)?),
        Command::WalletVerify {
            wallet,
            password_stdin,
        } => {
            let password = password(password_stdin, false)?;
            println!("{}", orvexa_wallet::unlock(&wallet, &password)?.address());
        }
        Command::Genesis {
            authority,
            recipient,
            out,
        } => {
            let genesis = Genesis::new(authority, recipient, Hash32(random_bytes()?));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out)
                .context("create genesis (existing files are never overwritten)")?;
            file.write_all(&serde_json::to_vec_pretty(&genesis)?)?;
            file.sync_all()?;
            print(
                &serde_json::json!({"chain_id": genesis.chain_id(), "supply_test_atoms": genesis.supply}),
            )?;
        }
        command => {
            let client = Client::new(&args.node)?;
            let status = client.status().await?;
            if let Some(expected) = args.expect_chain {
                ensure!(
                    status.chain_id == expected,
                    "node belongs to another network"
                );
            }
            match command {
                Command::Status => print(&status)?,
                Command::Network => print(
                    &client
                        .get::<orvexa_rpc::RuntimeStatus>("/v1/runtime")
                        .await?,
                )?,
                Command::Balance { address } => print(&client.balance(address).await?)?,
                Command::History { address } => print(&client.history(address).await?)?,
                Command::Send {
                    wallet,
                    to,
                    amount,
                    fee,
                    password_stdin,
                } => {
                    let password = password(password_stdin, false)?;
                    let key = orvexa_wallet::unlock(&wallet, &password)?;
                    let balance = client.balance(key.address()).await?;
                    let signed = orvexa_wallet::sign_transaction(
                        &key,
                        Transaction {
                            version: VERSION,
                            chain_id: status.chain_id,
                            sender: key.address(),
                            recipient: to,
                            amount,
                            fee,
                            nonce: balance.pending_nonce,
                        },
                    )?;
                    print(&client.submit(&signed).await?)?;
                }
                Command::Produce {
                    wallet,
                    password_stdin,
                } => {
                    let password = password(password_stdin, false)?;
                    let key = orvexa_wallet::unlock(&wallet, &password)?;
                    let genesis = client.genesis().await?;
                    ensure!(
                        genesis.chain_id() == status.chain_id,
                        "inconsistent node genesis"
                    );
                    let block =
                        orvexa_wallet::sign_block(&key, &genesis, client.template().await?)?;
                    print(&client.commit(&block).await?)?;
                }
                Command::Submit { file } => {
                    ensure!(
                        std::fs::metadata(&file)?.len() <= 8192,
                        "transaction file too large"
                    );
                    let tx: SignedTransaction = serde_json::from_slice(&std::fs::read(file)?)?;
                    print(&client.submit(&tx).await?)?;
                }
                Command::Sync { peer } => {
                    print(&orvexa_rpc::sync(&client, &Client::new(&peer)?).await?)?
                }
                _ => unreachable!("offline commands handled above"),
            }
        }
    }
    Ok(())
}
