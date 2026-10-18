import { useState, type InputHTMLAttributes } from 'react';
import { t } from './i18n';

// Keep the native file chooser and keyboard behavior, with app-language labels.
export default function FileInput({ onChange, ...props }: InputHTMLAttributes<HTMLInputElement> & { 'aria-label': string }) {
  const [name, setName] = useState('');
  return <span className="file-picker"><input {...props} type="file" onChange={event => {
    setName(event.target.files?.[0]?.name || ''); onChange?.(event);
  }}/><span className="file-picker-button" aria-hidden="true">{t('Choose file')}</span><span className="file-picker-name" aria-hidden="true">{name || t('No file selected')}</span></span>;
}
