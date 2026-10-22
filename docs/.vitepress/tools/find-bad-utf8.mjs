import fs from 'node:fs';

function findBad(buf) {
  const bad = [];
  let i = 0;
  while (i < buf.length) {
    const b = buf[i];
    let len = 0;
    if (b < 0x80) len = 1;
    else if ((b & 0xe0) === 0xc0) len = 2;
    else if ((b & 0xf0) === 0xe0) len = 3;
    else if ((b & 0xf8) === 0xf0) len = 4;
    else { bad.push([i, 1]); i++; continue; }
    let ok = true;
    for (let j = 1; j < len; j++) {
      if (i + j >= buf.length || (buf[i + j] & 0xc0) !== 0x80) { ok = false; break; }
    }
    if (!ok) { bad.push([i, len]); i++; } else i += len;
  }
  return bad;
}

for (const f of process.argv.slice(2)) {
  const buf = fs.readFileSync(f);
  const bad = findBad(buf);
  console.log(`== ${f} (bad=${bad.length})`);
  for (const [off] of bad) {
    const s = Math.max(0, off - 24), e = Math.min(buf.length, off + 24);
    console.log(`  offset=${off} ctx_hex=${buf.subarray(s, e).toString('hex')}`);
    console.log(`  ctx_ascii=${JSON.stringify(buf.subarray(s, e).toString('latin1'))}`);
  }
}