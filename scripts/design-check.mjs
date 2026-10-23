// Development-only browser checks. Uses the bundled Playwright; adds no runtime dependency.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, extname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const expectedName=/^name\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root,'brand/brand.toml'),'utf8'))[1];
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DM_PLAYWRIGHT||'C:/Users/PMLS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const {expandPreview,filterLinks,percent}=runInNewContext(readFileSync(join(root,'ui/helpers.js'),'utf8')+';({expandPreview,filterLinks,percent})');
assert.deepEqual(Array.from(expandPreview('file[001-003].jpg')),['file001.jpg','file002.jpg','file003.jpg']);
assert.deepEqual(Array.from(expandPreview('[1-5:2]')),['1','3','5']);assert.deepEqual(Array.from(expandPreview('[a-c]')),['a','b','c']);
assert.equal(expandPreview('[1-999999999]').length,500);assert.equal(expandPreview('[1-10:0]').length,1);
assert.equal(expandPreview('[1-2][a-b]').length,4);assert.equal(expandPreview('#comment\n\nhttps://a.test').length,1);
assert.equal(filterLinks('/zip/g')('file.zip'),true);assert.equal(filterLinks('/zip/g')('file.zip'),true);
assert.equal(percent({total:10,downloaded:12}),100);
assert.deepEqual(Array.from(expandPreview('[a-e:2]')),['a','c','e']);
assert.deepEqual(Array.from(expandPreview('[a-Z]')),['[a-Z]']);
for(const name of ['app','tray','tray-active','tray-paused']){const data=readFileSync(join(root,'brand/assets',name+'.ico'));assert.equal(data.readUInt16LE(2),1);assert.equal(data.readUInt16LE(4),5);assert.deepEqual(Array.from({length:5},(_,i)=>data[6+i*16]||256),[16,24,32,48,256])}
for(const [name,w,h] of [['wizard-large',164,314],['wizard-large@2x',328,628],['wizard-small',55,55],['wizard-small@2x',110,110]]){const data=readFileSync(join(root,'brand/assets',name+'.bmp'));assert.equal(data.readInt32LE(18),w);assert.equal(data.readInt32LE(22),h);assert.equal(data.readUInt16LE(28),24)}
for(const size of [16,32,48,128]){const data=readFileSync(join(root,`extension/icons/${size}.png`));assert.equal(data.readUInt32BE(16),size);assert.equal(data.readUInt32BE(20),size)}
const mime={'.html':'text/html','.css':'text/css','.js':'text/javascript','.svg':'image/svg+xml','.png':'image/png'};
const server=createServer((req,res)=>{const url=new URL(req.url,'http://127.0.0.1');if(url.pathname==='/fixture.bin'){res.writeHead(200,{'content-type':'application/octet-stream','content-length':1024});res.end(Buffer.alloc(1024,7));return}let rel=decodeURIComponent(url.pathname);if(rel==='/')rel='/ui/index.html';const path=resolve(root,'.'+rel);if(!path.startsWith(root+'\\')){res.writeHead(403).end();return}try{const data=readFileSync(path);res.writeHead(200,{'Content-Type':mime[extname(path)]||'application/octet-stream'});res.end(data)}catch{res.writeHead(404).end()}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({executablePath:process.env.DM_CHROME||'C:/Program Files/Google/Chrome/Application/chrome.exe',headless:true,args:['--disable-background-networking','--no-default-browser-check']});
const failures=[],requests=[],screenshots=[];
const capture=!process.argv.includes('--quick');
const context=await browser.newContext({reducedMotion:'reduce'});const page=await context.newPage();
page.on('pageerror',e=>failures.push(e.message));page.on('request',r=>requests.push({url:r.url(),method:r.method(),type:r.resourceType()}));
page.on('console',message=>{if(message.type()==='error')failures.push(message.text())});
const shots=join(root,'docs/screenshots');mkdirSync(shots,{recursive:true});
async function shot(name){await page.screenshot({path:join(shots,name+'.png'),fullPage:!await page.evaluate(()=>!!document.querySelector('#dialog')?.open)});screenshots.push(name+'.png')}
async function goto(query=''){await page.goto(origin+'/ui/index.html?mock=1'+query);await page.locator('.shell').waitFor();}
try{
 await goto();assert.equal(await page.locator('tr[data-id]').count(),8);assert.equal(await page.locator('.brand').innerText(),expectedName);assert.equal(await page.locator('.fileicon svg').count(),8);assert.equal(await page.locator('.fileicon').first().innerText(),'');await page.locator('[data-action="sort:name"]').click();assert.equal(await page.locator('[data-action="sort:name"] svg').count(),1);
 await page.locator('[data-action=add]').focus();await page.keyboard.press('Enter');assert.equal(await page.locator('[name=urls]').evaluate(e=>e===document.activeElement),true);
 assert.equal(await page.locator('.dialog-head [data-action=close] svg[data-icon=close] path').count(),1);assert.equal(await page.locator('.dialog-head [data-action=close]').innerText(),'');for(let i=0;i<25;i++){await page.keyboard.press('Tab');assert.equal(await page.evaluate(()=>document.querySelector('dialog').contains(document.activeElement)),true)}await page.keyboard.press('Escape');assert.equal(await page.locator('#dialog').evaluate(e=>e.open),false);
 await page.locator('[data-select="1"]').check();await page.locator('[data-action="bulk:pause"]').click();assert.match(await page.locator('tr[data-id="1"]').innerText(),/Paused/);
 await page.locator('[data-action="bulk:resume"]').click();assert.match(await page.locator('tr[data-id="1"]').innerText(),/Queued/);
 await page.locator('[data-action="add"]').click();await page.locator('[name=urls]').fill('https://example.test/file[001-003].zip');assert.match(await page.locator('#expansion').innerText(),/3 links/);await page.locator('[data-action=submitAdd]').click();await page.waitForFunction(()=>!document.querySelector('dialog').open);assert.equal(await page.locator('tr[data-id]').count(),11);
 await page.locator('[data-action="add"]').click();await page.locator('[name=urls]').fill('https://example.test/file001.zip\ninvalid');await page.locator('[data-action=submitAdd]').click();assert.match(await page.locator('#add-result').innerText(),/Invalid lines: invalid/);await page.keyboard.press('Escape');
 await goto('&screen=settings');await page.locator('[name=download_root]').fill('C:\\Preview\\Test');await page.locator('[data-action=saveSettings]').click();assert.equal(await page.locator('[name=download_root]').inputValue(),'C:\\Preview\\Test');
 await goto('&screen=queues');await page.locator('[data-action=newQueue]').click();await page.locator('[name=name]').fill('Weekend');await page.locator('[name=scheduled]').check();await page.locator('[name=start]').fill('23:00');await page.locator('[name=stop]').fill('06:00');await page.locator('[data-action=saveQueue]').click();assert.equal(await page.locator('[name=name]').inputValue(),'Weekend');
 await goto('&state=clipboard');await page.locator('[data-action=clipNever]').click();assert.equal(await page.locator('[data-action=clipYes]').count(),0);
 await goto('&screen=grabber');await page.locator('[data-action=grabAll]').click();await page.locator('[data-action=grabDownload]').click();assert.match(await page.locator('[name=urls]').inputValue(),/https:/);await page.keyboard.press('Escape');
 await goto();await page.locator('tr[data-id="1"]').focus();await page.keyboard.press('Enter');assert.equal(await page.locator('#detail-progress').count(),1);
 await goto();await page.locator('#search').fill('nothing-matches');await page.getByRole('heading',{name:'No matching downloads'}).waitFor();assert.equal(await page.locator('[data-clear=search] svg[data-icon=close] path').count(),1);assert.equal(await page.locator('[data-clear=search]').innerText(),'');await page.locator('[data-clear=search]').click();await page.waitForFunction(()=>document.querySelectorAll('tr[data-id]').length===8);assert.equal(await page.locator('#search').inputValue(),'');
 const forbiddenIcons=/[\u00d7\u2715\u2716\u274c\u25b2\u25bc\u25b6\u23f8]/u;assert.equal(forbiddenIcons.test(await page.locator('body').innerText()),false);
 // Keep the transfer table dominant and dense at ordinary desktop and narrow sizes.
 await page.setViewportSize({width:900,height:900});await goto('&state=many');assert.equal(await page.locator('.stats').count(),0);assert.equal(await page.locator('tr[data-id]').count(),200);assert((await page.locator('tr[data-id]').first().boundingBox()).height<=38);assert((await page.locator('.tablewrap').boundingBox()).height>=500);assert.equal(await page.locator('.toolbar [data-action=add] svg').count(),1);
 await page.setViewportSize({width:360,height:800});await goto();assert.equal(await page.locator('.tasks').evaluate(e=>getComputedStyle(e).display),'table');assert.equal(await page.locator('.tasks thead').isVisible(),true);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
 const routes=[['main',''],['add','&screen=add'],['add-many','&screen=add'],['add-file','&screen=add'],['details','&screen=details'],['details-error',''],['no-results',''],['grabber-empty','&screen=grabber'],['queues','&screen=queues'],...['general','connections','folders','exclusions','streams','advanced','about'].map(tab=>['settings-'+tab,'&screen=settings&tab='+tab]),['history','&screen=history'],['grabber','&screen=grabber'],['welcome','&screen=welcome'],...['empty','loading','error','many','clipboard','toast'].map(state=>[state,'&state='+state]),['confirm-remove',''],['confirm-files',''],['confirm-shutdown','&screen=queues']];
 if(capture)for(const theme of ['light','dark'])for(const width of [360,900,1600]){
   await page.setViewportSize({width,height:width===360?800:900});await page.evaluate(theme=>localStorage.setItem('theme',theme),theme);
   for(const [name,query] of routes){await goto(query);
    if(name==='details-error'){await page.locator('tr[data-id=\"7\"]').dblclick()}
    if(name==='no-results'){await page.locator('#search').fill('nothing-matches-this');await page.getByRole('heading',{name:'No matching downloads'}).waitFor()}
    if(name==='grabber-empty')await page.locator('[name=grabText]').fill('');
    if(name==='add-many')await page.locator('[data-action="addtab:many"]').click();
    if(name==='add-file')await page.locator('[data-action="addtab:file"]').click();
    if(name.startsWith('confirm-')&&name!=='confirm-shutdown'){await page.locator('[data-select="1"]').check();await page.locator('[data-action=remove]').click();if(name==='confirm-files')await page.locator('[name=delete-files]').check()}
    if(name==='confirm-shutdown'){await page.locator('[name=on_finish]').selectOption('Shutdown');await page.locator('[data-action=saveQueue]').click()}
    const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth);if(overflow)failures.push(`Overflow ${name} ${theme} ${width}`);
    await shot(`${name}-${theme}-${width}`);
   }
   await goto('&dir=rtl');await shot(`main-rtl-${theme}-${width}`);
   const ratios=await page.evaluate(()=>{const style=getComputedStyle(document.documentElement),canvas=document.createElement('canvas'),ctx=canvas.getContext('2d');canvas.width=canvas.height=1;const luminance=token=>{ctx.fillStyle=style.getPropertyValue(token);ctx.fillRect(0,0,1,1);const channels=[...ctx.getImageData(0,0,1,1).data].slice(0,3).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4});return channels.reduce((n,v,i)=>n+v*[.2126,.7152,.0722][i],0)};return [['--text','--panel',4.5],['--muted','--bg',4.5],['--accent','--soft',4.5],['--good','--soft',4.5],['--bad','--soft',4.5],['--control','--panel',3]].map(([a,b,min])=>{const values=[luminance(a),luminance(b)].sort((a,b)=>a-b);return{a,b,min,ratio:(values[1]+.05)/(values[0]+.05)}})});ratios.forEach(r=>assert(r.ratio>=r.min,`${theme} contrast ${JSON.stringify(r)}`));
   for(const [name,query] of routes){await goto(query+'&dir=rtl');assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`RTL overflow ${name} ${width}`)}
   await page.goto(origin+'/extension/popup.html?mock=1');await page.locator('#list input').first().waitFor();await shot(`popup-${theme}-${width}`);
   for(const state of ['app_not_running','missing','version_mismatch','empty']){await page.locator('#preview-state').selectOption(state);await shot(`popup-${state}-${theme}-${width}`)}
 }
 await page.goto(origin+'/extension/popup.html?mock=1');assert.equal(forbiddenIcons.test(await page.locator('body').innerText()),false);await page.locator('#all').click();assert.equal(await page.locator('#go').isEnabled(),true);await page.locator('#go').click();assert.match(await page.locator('#count').innerText(),/3 added/);
 // Exercise the unchanged chrome message protocol, independently of mock mode.
 const popupPage=await context.newPage();popupPage.on('pageerror',e=>failures.push(e.message));popupPage.on('request',r=>requests.push({url:r.url(),method:r.method(),type:r.resourceType()}));popupPage.on('console',m=>{if(m.type()==='error')failures.push(m.text())});await popupPage.addInitScript(()=>{
   window.__calls=[];
   window.chrome={runtime:{async sendMessage(msg){window.__calls.push(msg);if(msg.type==='status')return{brand:'Test identity',capture:true,version:'0.1.0',status:{ok:true}};if(msg.type==='media')return{media:[{kind:'hls',name:'Test stream',url:'https://example.test/stream.m3u8',size:2000000}]};return{ok:true}}},tabs:{async query(){return[{id:12,url:'https://example.test/page',title:'Test page'}]}},scripting:{async executeScript(){return[{result:[{url:'https://example.test/link.zip',text:'Test link'}]}]}},storage:{session:{async get(){return{}},async remove(){}}}};
 });await popupPage.goto(origin+'/extension/popup.html');await popupPage.locator('#media button').click();assert.equal(await popupPage.locator('#media button svg[data-icon=check] path').count(),1);await popupPage.locator('#capture').uncheck();await popupPage.locator('#links').click();await popupPage.locator('#all').click();await popupPage.locator('#go').click();await popupPage.locator('#openUi').click();const calls=await popupPage.evaluate(()=>window.__calls);assert.deepEqual(calls.map(c=>c.type),['status','media','add','set_capture','add','open_ui']);assert.deepEqual(calls[2],{type:'add',urls:['https://example.test/stream.m3u8'],referer:'https://example.test/page',filename:'Test page'});assert.deepEqual(calls[4],{type:'add',urls:['https://example.test/link.zip'],referer:'https://example.test/page'});await popupPage.close();
 const filePage=await context.newPage();await filePage.goto(new URL('../ui/index.html?mock=1',import.meta.url).href);await filePage.locator('tr[data-id="1"]').waitFor();assert.equal(await filePage.locator('tr[data-id]').count(),8);await filePage.close();
 assert.equal(requests.filter(r=>new URL(r.url).hostname!=='127.0.0.1').length,0);
 const folderSize=dir=>readdirSync(dir).reduce((sum,file)=>sum+(statSync(join(dir,file)).isDirectory()?folderSize(join(dir,file)):statSync(join(dir,file)).size),0);
 const uiBytes=folderSize(join(root,'ui')),popupBytes=['popup.html','popup.css','popup.js','popup-brand.css','popup-strings.js','popup-icons.js','lib.js','brand.js','icons/16.png','icons/32.png'].reduce((n,f)=>n+statSync(join(root,'extension',f)).size,0);assert(uiBytes<300000);assert(popupBytes<60000);
 const previous=capture?null:JSON.parse(readFileSync(join(root,'docs/design-browser-log.json'),'utf8'));
 writeFileSync(join(root,'docs/design-browser-log.json'),JSON.stringify({date:'2026-10-24',uiBytes,popupBytes,requests:previous?[...previous.requests,...requests]:requests,failures,screenshots:previous?previous.screenshots:screenshots},null,2));
 assert.deepEqual(failures,[]);console.log(JSON.stringify({uiBytes,popupBytes,screenshots:screenshots.length,requests:requests.length,failures}));
}finally{await browser.close();server.close()}
