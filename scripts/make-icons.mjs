// Reproduce all identity assets from brand/brand.toml. No packages required.
// Run with: node scripts/make-icons.mjs [extension-icon-output-directory]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { deflateSync, crc32 } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const toml=readFileSync(join(root,'brand/brand.toml'),'utf8');
const brand=Object.fromEntries(['name','company','homepage','primary_color'].map(key=>{const value=new RegExp(`^${key}\\s*=\\s*"([^"]*)"`,'m').exec(toml)?.[1];if(!value)throw Error(`Missing ${key}`);return[key,value]}));
if(!/^#[a-f\d]{6}$/i.test(brand.primary_color))throw Error('primary_color must be a six-digit hex colour');
const rgb=[1,3,5].map(i=>parseInt(brand.primary_color.slice(i,i+2),16));
const asset=join(root,'brand/assets'),icons=process.argv[2]||join(root,'extension/icons');
for(const path of [asset,icons])mkdirSync(path,{recursive:true});
const write=(file,data)=>writeFileSync(join(root,file),data);
const xml=s=>s.replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&apos;'}[c]));
const path='M4 4H10V28H4Z M13 4H19L28 13V19L19 28H13V22H16L22 16L16 10H13Z';
const wordmarkWidth=44+Math.ceil(brand.name.length*15);
const svg=(color,wide=false)=>`<svg xmlns="http://www.w3.org/2000/svg" width="${wide?wordmarkWidth:32}" height="32" viewBox="0 0 ${wide?wordmarkWidth:32} 32" role="img" aria-label="${xml(brand.name)}"><path fill="${color}" d="${path}"/>${wide?`<text x="44" y="24" font-family="Segoe UI,system-ui,sans-serif" font-size="23" font-weight="650" fill="${color}">${xml(brand.name)}</text>`:''}</svg>\n`;
write('brand/assets/logo.svg',svg(brand.primary_color));write('brand/assets/logo-white.svg',svg('#ffffff'));write('brand/assets/logo-wide.svg',svg(brand.primary_color,true));write('brand/assets/logo-wide-white.svg',svg('#ffffff',true));write('ui/mark.svg',svg(brand.primary_color));
write('brand/assets/identity-preview.svg',`<svg xmlns="http://www.w3.org/2000/svg" width="720" height="360" viewBox="0 0 720 360" role="img" aria-label="${xml(brand.name)} identity"><path fill="#ffffff" d="M0 0h720v180H0z"/><path fill="#10151e" d="M0 180h720v180H0z"/>${[0,180].map((y,i)=>`<g transform="translate(60 ${y+42})"><g transform="scale(3)"><path fill="${i?'#ffffff':brand.primary_color}" d="${path}"/></g><text x="132" y="65" fill="${i?'#ffffff':'#182235'}" font-family="Segoe UI,system-ui,sans-serif" font-size="52" font-weight="650" letter-spacing="-1.5">${xml(brand.name)}</text><text x="134" y="96" fill="${i?'#a3b1c7':'#58677d'}" font-family="Segoe UI,system-ui,sans-serif" font-size="13" letter-spacing="2">DOWNLOAD MANAGER</text></g>`).join('')}</svg>\n`);
write('ui/brand.js',`// Generated from brand/brand.toml by scripts/make-icons.mjs.\nconst brand = ${JSON.stringify(brand,null,2)};\n`);
for(const file of ['ui/brand.css','extension/popup-brand.css'])write(file,`/* Generated from brand/brand.toml. */\n:root{--brand:${brand.primary_color}}\n`);
// Popup has its own copy of the shared translation object, so it stays self-contained.
write('extension/popup-icons.js',readFileSync(join(root,'ui/icons.js'),'utf8').replace('const uiIcon =','export const uiIcon ='));
const host=/export const HOST = ([^;]+);/.exec(readFileSync(join(root,'extension/brand.js'),'utf8'))[1];
write('extension/brand.js',`// Generated from brand/brand.toml. Native-host identity is unchanged.\nexport const BRAND_NAME = ${JSON.stringify(brand.name)};\nexport const HOST = ${host};\n`);
write('extension/_locales/en/messages.json',JSON.stringify({extName:{message:brand.name},extDesc:{message:`Fast download manager for ${brand.name}: catches downloads, finds videos and grabs page links.`}},null,2)+'\n');
write('extension/popup-strings.js',readFileSync(join(root,'ui/strings.js'),'utf8').replace('const S =','export const S ='));
const outline=[[13,4],[19,4],[28,13],[28,19],[19,28],[13,28],[13,22],[16,22],[22,16],[16,10],[13,10]];
const mark=(x,y)=>{if(x>=4&&x<10&&y>=4&&y<28)return true;let inside=false;for(let i=0,j=outline.length-1;i<outline.length;j=i++){const [ax,ay]=outline[i],[bx,by]=outline[j];if((ay>y)!==(by>y)&&x<(bx-ax)*(y-ay)/(by-ay)+ax)inside=!inside}return inside};
if(!mark(6,16)||!mark(25,16)||mark(12,16)||mark(1,1))throw Error('Monogram geometry check failed');
function rgba(size,mode='app'){
 const out=Buffer.alloc(size*size*4);const aa=4;
 for(let y=0;y<size;y++)for(let x=0;x<size;x++){
   const sums=[0,0,0,0];
   for(let sy=0;sy<aa;sy++)for(let sx=0;sx<aa;sx++){
     const fx=(x+(sx+.5)/aa)/size*32,fy=(y+(sy+.5)/aa)/size*32;
     const dx=Math.max(Math.abs(fx-16)-9,0),dy=Math.max(Math.abs(fy-16)-9,0),inside=dx*dx+dy*dy<36;
     let pixel=[0,0,0,0];
     if(mode==='tray'){if(mark(fx,fy))pixel=[...rgb,255]}
     else if(inside){const on=mode==='paused'?(fx>=10&&fx<14||fx>=18&&fx<22)&&fy>=8&&fy<24:mark(fx,fy);pixel=on?[255,255,255,255]:[...rgb,255]}
     for(let c=0;c<4;c++)sums[c]+=pixel[c];
   }
   const i=(y*size+x)*4;out[i+3]=Math.round(sums[3]/(aa*aa));for(let c=0;c<3;c++)out[i+c]=sums[3]?Math.round(sums[c]*255/sums[3]):0;
 }return out;
}
function chunk(type,data){const t=Buffer.from(type),len=Buffer.alloc(4),crc=Buffer.alloc(4);len.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([t,data])));return Buffer.concat([len,t,data,crc])}
function png(size,mode){const pixels=rgba(size,mode),raw=Buffer.alloc(size*(size*4+1));for(let y=0;y<size;y++)pixels.copy(raw,y*(size*4+1)+1,y*size*4,(y+1)*size*4);const header=Buffer.alloc(13);header.writeUInt32BE(size);header.writeUInt32BE(size,4);header.set([8,6],8);return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(raw)),chunk('IEND',Buffer.alloc(0))])}
function ico(mode){const sizes=[16,24,32,48,256],images=sizes.map(s=>png(s,mode));const header=Buffer.alloc(6+16*sizes.length);header.writeUInt16LE(1,2);header.writeUInt16LE(sizes.length,4);let offset=header.length;sizes.forEach((s,i)=>{const p=6+i*16;header[p]=header[p+1]=s===256?0:s;header.writeUInt16LE(1,p+4);header.writeUInt16LE(32,p+6);header.writeUInt32LE(images[i].length,p+8);header.writeUInt32LE(offset,p+12);offset+=images[i].length});return Buffer.concat([header,...images])}
for(const size of [16,32,48,128])writeFileSync(join(icons,`${size}.png`),png(size,'app'));
for(const [name,mode] of [['app','app'],['tray','tray'],['tray-active','app'],['tray-paused','paused']])write(`brand/assets/${name}.ico`,ico(mode));
function bmp(w,h){const stride=Math.ceil(w*3/4)*4,pixels=Buffer.alloc(stride*h),header=Buffer.alloc(54),scale=w/164;
 for(let y=0;y<h;y++)for(let x=0;x<w;x++){
  const fx=x/w,fy=y/h;let color=rgb.map(c=>Math.round(c*(.88+.12*(1-fy))));
  const tile=w===h?w*.72:w*.65,ox=(w-tile)/2,oy=w===h?(h-tile)/2:55*scale;
  if(mark((x-ox)/tile*32,(y-oy)/tile*32))color=[255,255,255];
  if(w!==h&&y>h*.58){const bx=(x/(28*scale))%1,by=(y/(28*scale))%1;if((bx<.12||by<.12)&&((x+y)/(w+h)>.5))color=color.map(c=>Math.round(c*.7+255*.3))}
  const i=(h-1-y)*stride+x*3;pixels.set([...color].reverse(),i);
 }header.write('BM');header.writeUInt32LE(54+pixels.length,2);header.writeUInt32LE(54,10);header.writeUInt32LE(40,14);header.writeInt32LE(w,18);header.writeInt32LE(h,22);header.writeUInt16LE(1,26);header.writeUInt16LE(24,28);header.writeUInt32LE(pixels.length,34);return Buffer.concat([header,pixels])}
for(const [name,w,h] of [['wizard-large',164,314],['wizard-large@2x',328,628],['wizard-small',55,55],['wizard-small@2x',110,110]])write(`brand/assets/${name}.bmp`,bmp(w,h));
console.log('Brand assets, manager tokens and popup copy generated.');
