import fs from 'node:fs';

for (const f of process.argv.slice(2)) {
  const buf = fs.readFileSync(f);
  const dec = new TextDecoder('utf-8', { fatal: true });
  let ok = true, err = '';
  try { dec.decode(buf); } catch (e) { ok = false; err = e.message; }
  const txt = buf.toString('utf8');
  const repl = (txt.match(/\uFFFD/g) || []).length;
  const bom = buf[0] === 0xef && buf[1] === 0xbb && buf[2] === 0xbf;
  const crlf = (txt.match(/\r\n/g) || []).length;
  console.log(`${f} | bytes=${buf.length} utf8_valid=${ok} bom=${bom} replacement_chars=${repl} crlf=${crlf}${err ? ' ERR=' + err : ''}`);
}