/// <reference lib="webworker" />
const context = self as unknown as DedicatedWorkerGlobalScope;
context.onmessage = async (event: MessageEvent<{ operation: string; args: string[] }>) => {
  try {
    const path = '/wasm/orvexa_web_wallet.js';
    const wasm = await import(/* @vite-ignore */ path);
    await wasm.default();
    const allowed = ['create_wallet', 'inspect_wallet', 'authenticate_wallet', 'validate_address', 'verify_network', 'sign_transfer', 'authorize_block'];
    if (!allowed.includes(event.data.operation)) throw new Error('Unsupported local signer operation.');
    const result = wasm[event.data.operation](...event.data.args);
    context.postMessage({ result });
  } catch (error) {
    // Errors from the Rust adapter contain no passwords, keys or wallet bytes.
    context.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};
