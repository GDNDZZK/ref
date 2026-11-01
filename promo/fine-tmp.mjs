// node fine-tmp.mjs in out x0 y0 x1 y1 : crop (fractions) with a 1% grid labelled every 5%
import sharp from 'sharp'
const [src, dst, ...r] = process.argv.slice(2)
const [x0, y0, x1, y1] = r.map(Number)
const { width: W, height: H } = await sharp(src).metadata()
const left = Math.round(x0 * W), top = Math.round(y0 * H), w = Math.round((x1 - x0) * W), h = Math.round((y1 - y0) * H)
let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">`
for (let p = Math.ceil(x0 * 100); p <= x1 * 100; p++) { const x = (p / 100) * W - left; svg += `<line x1="${x}" y1="0" x2="${x}" y2="${h}" stroke="${p % 5 ? '#80008060' : '#ff00ff'}"/>`; if (p % 5 === 0) svg += `<text x="${x + 2}" y="14" fill="#ff0" font-size="14">${p / 100}</text>` }
for (let p = Math.ceil(y0 * 100); p <= y1 * 100; p++) { const y = (p / 100) * H - top; svg += `<line x1="0" y1="${y}" x2="${w}" y2="${y}" stroke="${p % 5 ? '#80008060' : '#ff00ff'}"/>`; if (p % 5 === 0) svg += `<text x="2" y="${y - 2}" fill="#ff0" font-size="14">${p / 100}</text>` }
svg += '</svg>'
await sharp(src).extract({ left, top, width: w, height: h }).composite([{ input: Buffer.from(svg) }]).toFile(dst)
