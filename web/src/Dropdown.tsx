import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Check, ChevronDown } from 'lucide-react';

export interface DropdownOption { value: string; label: string; hint?: string; icon?: ReactNode }

/**
 * App-drawn dropdown used instead of the OS <select>, so it looks and behaves
 * the same on macOS and Windows. The whole button opens it; keyboard: arrows,
 * Home/End, Enter/Space, Escape, and typing a letter jumps to an option.
 */
export default function Dropdown({ label, value, options, onChange, placeholder, leading, className = '', align = 'start' }: {
  label: string; value: string; options: DropdownOption[]; onChange: (value: string) => void;
  placeholder?: string; leading?: ReactNode; className?: string; align?: 'start' | 'end';
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null); const button = useRef<HTMLButtonElement>(null); const list = useRef<HTMLUListElement>(null);
  const id = useId();
  const selected = options.find(o => o.value === value);
  const show = (index: number) => { setActive(Math.max(0, index)); setOpen(true); };
  const choose = (index: number) => { const o = options[index]; if (o) onChange(o.value); setOpen(false); button.current?.focus(); };

  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', away);
    return () => document.removeEventListener('pointerdown', away);
  }, [open]);
  useEffect(() => { if (open) list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' }); }, [open, active]);

  const key = (e: KeyboardEvent) => {
    const current = options.findIndex(o => o.value === value);
    if (!open) {
      if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) { e.preventDefault(); show(current < 0 ? 0 : current); }
      return;
    }
    if (e.key === 'Escape' || e.key === 'Tab') { if (e.key === 'Escape') e.preventDefault(); setOpen(false); return; }
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive(a => Math.min(options.length - 1, a + 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(a => Math.max(0, a - 1)); }
    else if (e.key === 'Home') { e.preventDefault(); setActive(0); }
    else if (e.key === 'End') { e.preventDefault(); setActive(options.length - 1); }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(active); }
    else if (e.key.length === 1) {
      const from = options.findIndex((o, i) => i > active && o.label.toLowerCase().startsWith(e.key.toLowerCase()));
      const any = from >= 0 ? from : options.findIndex(o => o.label.toLowerCase().startsWith(e.key.toLowerCase()));
      if (any >= 0) setActive(any);
    }
  };

  return <div ref={root} className={`dropdown ${open ? 'open' : ''} ${className}`}>
    <button ref={button} type="button" className="dropdown-button" role="combobox" aria-label={label} aria-haspopup="listbox" aria-expanded={open} aria-controls={`${id}-list`}
      aria-activedescendant={open ? `${id}-${active}` : undefined}
      onClick={() => open ? setOpen(false) : show(Math.max(0, options.findIndex(o => o.value === value)))} onKeyDown={key}>
      {leading}<span className="dropdown-value">{selected?.label ?? placeholder ?? ''}</span><ChevronDown size={15} className="dropdown-chevron" aria-hidden="true"/>
    </button>
    {open && <ul ref={list} id={`${id}-list`} className={`dropdown-list align-${align}`} role="listbox" aria-label={label}>
      {options.map((o, i) => <li key={o.value} id={`${id}-${i}`} data-index={i} role="option" aria-selected={o.value === value}
        className={`${i === active ? 'active' : ''} ${o.value === value ? 'selected' : ''}`}
        onPointerEnter={() => setActive(i)} onPointerDown={e => e.preventDefault()} onClick={() => choose(i)}>
        {o.icon}<span className="dropdown-option-text"><span>{o.label}</span>{o.hint && <small>{o.hint}</small>}</span>{o.value === value && <Check size={15} className="dropdown-check" aria-hidden="true"/>}
      </li>)}
    </ul>}
  </div>;
}
