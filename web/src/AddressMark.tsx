// A small, deterministic picture of an address, drawn in the ORVEXA petal
// geometry. It helps people recognise an address at a glance; it is never a
// substitute for comparing the full address before signing.
// Only palette-derived tones: navy through steel, never a new hue.
const shades = ['#1B2B3D', '#24384F', '#334B65', '#16263A', '#2B4560', '#3E5A78'];

function bytes(address: string) {
  const hex = address.replace(/^void_test_/, '').replace(/[^a-f0-9]/gi, '').padEnd(24, '0');
  const out: number[] = [];
  for (let i = 0; i < 24; i += 2) out.push(parseInt(hex.slice(i, i + 2), 16) || 0);
  return out;
}

export default function AddressMark({ address, size = 40, className = '' }: { address: string; size?: number; className?: string }) {
  const b = bytes(address);
  const turn = (b[0] % 60);
  const background = shades[b[1] % shades.length];
  return <svg className={`address-mark ${className}`} width={size} height={size} viewBox="-20 -20 40 40" aria-hidden="true" focusable="false">
    <circle r="20" fill={background}/>
    {Array.from({ length: 6 }, (_, i) => {
      // Each petal: its own length, and either solid or a quieter tone.
      const length = 5 + (b[2 + i] % 12);
      const quiet = (b[8 + (i % 4)] >> i) & 1;
      return <rect key={i} x={-2.7} y={-length - 2.5} width={5.4} height={length} rx={0.7}
        fill="#E6F2FF" fillOpacity={quiet ? 0.38 : 1} transform={`rotate(${turn + i * 60})`}/>;
    })}
    <circle r="3.6" fill={background}/>
    <circle r="1.5" fill="#E6F2FF"/>
  </svg>;
}
