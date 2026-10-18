import JSONbig from 'json-bigint';

const lossless = JSONbig({ alwaysParseAsBig: true, useNativeBigInt: true, protoAction: 'error', constructorAction: 'error' });
/** Normalize both small and large JSON integers to exact decimal strings. */
export const parseNodeJson = <T>(raw: string): T => lossless.parse(raw, (_key: string, value: unknown) => typeof value === 'bigint' ? value.toString() : value) as T;
