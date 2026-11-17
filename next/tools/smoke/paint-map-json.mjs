// Rust owns u64 fragment identities. Keep their exact JSON integer spelling
// when a Node audit indexes them; Number would merge neighboring micro masks.
export function parsePaintMap(bytes) {
  return JSON.parse(String(bytes).replace(/("fragment"\s*:\s*)(\d+)/g, '$1"$2"'));
}
