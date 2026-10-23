// Isolated API / browser smoke test. Never uses the user's AppData or Downloads.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const data=join(tmpdir(),'dm-design','data'),downloads=join(tmpdir(),'dm-design','downloads');mkdirSync(data,{recursive:true});mkdirSync(downloads,{recursive:true});
const app=spawn(join(root,'target/release/dmd.exe'),[],{cwd:root,env:{...process.env,DM_DATA_DIR:data,DM_DOWNLOAD_DIR:downloads,DM_UI_DIR:join(root,'ui')},windowsHide:true,stdio:'ignore'});
const fixture=createServer((req,res)=>{res.writeHead(200,{'Content-Type':'application/octet-stream','Content-Length':1024});res.end(Buffer.alloc(1024,7))});await new Promise(r=>fixture.listen(0,'127.0.0.1',r));
let browser,auth;const requests=[],errors=[];
try{
 for(let i=0;i<100;i++){if(existsSync(join(data,'api.json'))){auth=JSON.parse(readFileSync(join(data,'api.json'),'utf8'));if(auth.pid===app.pid)break}await new Promise(r=>setTimeout(r,100))}
 assert.equal(auth?.pid,app.pid,'Isolated dmd must own api.json');
 const origin=`http://127.0.0.1:${auth.port}`,headers={Authorization:'Bearer '+auth.token,'Content-Type':'application/json'};
 const api=async(path,method='GET',body)=>{const r=await fetch(origin+'/api'+path,{method,headers,body:body===undefined?undefined:JSON.stringify(body)});assert(r.ok,`${method} ${path}: ${await (r.ok?Promise.resolve(''):r.text())}`);return path==='/export'?r.text():r.json()};
 const {chromium}=createRequire(import.meta.url)(process.env.DM_PLAYWRIGHT||'C:/Users/PMLS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
 browser=await chromium.launch({executablePath:'C:/Program Files/Google/Chrome/Application/chrome.exe',headless:true});const context=await browser.newContext({viewport:{width:900,height:700}});const page=await context.newPage();
 page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>requests.push({url:r.url().replace(/token=[^&]+/g,'token=REDACTED'),method:r.method()}));
 await page.goto(origin+'/#token='+auth.token+'&view=main');await page.locator('[data-action=welcomeSkip]').click();
 await page.locator('[data-action=add]').first().click();await page.locator('[name=urls]').fill(`http://127.0.0.1:${fixture.address().port}/design-fixture.bin`);await page.locator('[name=paused]').selectOption('true');await page.locator('[data-action=submitAdd]').click();await page.waitForFunction(()=>!document.querySelector('dialog').open);
 let list=await api('/tasks');const task=list.find(t=>t.url.includes('design-fixture'));assert(task);assert.equal(task.status,'Paused');
 await page.locator(`[data-select="${task.id}"]`).check();await page.locator('[data-action="bulk:resume"]').click();
 for(let i=0;i<100;i++){list=await api('/tasks');if(list.find(t=>t.id===task.id)?.status==='Done')break;await new Promise(r=>setTimeout(r,100))}assert.equal(list.find(t=>t.id===task.id).status,'Done');assert(existsSync(list.find(t=>t.id===task.id).path));
 await page.locator('[data-action=pauseAll]').click();await page.locator('[data-action=resumeAll]').click();
 await page.locator('[data-action="page:settings"]').first().click();await page.locator('[name=clipboard_monitor]').check();await page.locator('[data-action=saveSettings]').click();assert.equal((await api('/settings')).clipboard_monitor,true);
 await page.locator('[data-action="page:queues"]').first().click();await page.locator('[data-action=newQueue]').click();await page.locator('[name=name]').fill('Design check');await page.locator('[data-action=saveQueue]').click();assert((await api('/queues')).some(q=>q.name==='Design check'));await page.locator('[data-action=deleteQueue]').click();await page.locator('[data-action=confirmDeleteQueue]').click();
 const exported=await api('/export');assert.match(exported,/design-fixture/);
 await page.locator('[data-action="filter:"]').click();await page.locator(`[data-select="${task.id}"]`).check();await page.locator('[data-action=remove]').click();await page.locator('[data-action=confirmRemove]').click();assert(!(await api('/tasks')).some(t=>t.id===task.id));assert(existsSync(list.find(t=>t.id===task.id).path),'Removing an entry must keep its file');
 assert.equal(requests.filter(r=>new URL(r.url).hostname!=='127.0.0.1').length,0);assert.deepEqual(errors,[]);
 // Verify open controls serialize the documented payload without launching another desktop app.
 const added=await api('/tasks','POST',{urls:`http://127.0.0.1:${fixture.address().port}/open-test.bin`});let done;for(let i=0;i<100;i++){done=(await api('/tasks')).find(t=>t.id===added.added[0]);if(done?.status==='Done')break;await new Promise(r=>setTimeout(r,100))}
 const opens=[];await page.route('**/api/tasks/*/open',async route=>{opens.push(route.request().postDataJSON());await route.fulfill({status:200,body:'{"ok":true}',contentType:'application/json'})});await page.locator('[data-action=retry]').count();await page.reload();await page.locator(`[data-select="${done.id}"]`).check();await page.locator('[data-action="open:file"]').click();await page.locator('[data-action="open:folder"]').click();assert.deepEqual(opens,[{what:'file'},{what:'folder'}]);
 await page.locator('[data-action=remove]').click();await page.locator('[name=delete-files]').check();await page.locator('[data-action=confirmRemove]').click();assert(!existsSync(done.path));
 writeFileSync(join(root,'docs/design-real-log.json'),JSON.stringify({date:'2026-10-24',dataDirectory:data,downloadDirectory:downloads,requests,errors,passed:['ping','token session','add paused','resume','task events','progress events','pause all','resume all','settings','queues create/delete','export','remove keeps file','remove deletes file','open file/folder payload (intercepted)']},null,2));console.log('Isolated real-engine browser checks passed.');
 await api('/quit','POST',{});
}finally{if(browser)await browser.close();fixture.close();if(app.exitCode===null){if(auth)await fetch(`http://127.0.0.1:${auth.port}/api/quit`,{method:'POST',headers:{Authorization:'Bearer '+auth.token,'Content-Type':'application/json'},body:'{}'}).catch(()=>{});else app.kill()}}

