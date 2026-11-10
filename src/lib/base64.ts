/** Decode a standard-alphabet base64 string (as emitted by the Rust
 * `base64` crate, which always pads) into bytes. `atob` yields a binary
 * string, which we widen byte-by-byte into a `Uint8Array` — this is the
 * frontend counterpart to `src-tauri/src/commands/mod.rs::base64_bytes`.
 */
export function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return bytes;
}
