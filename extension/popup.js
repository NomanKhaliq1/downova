import { collectFromPage, fileName, formatSize, makeFilter } from './lib.js';
import { uiIcon } from './popup-icons.js';
import { S } from './popup-strings.js';
const T=S.popup;
const $=id=>document.getElementById(id);
const mock=new URLSearchParams(location.search).get('mock')==='1';
let tab=null,found=[],previewState=new URLSearchParams(location.search).get('state')||'connected';
const ask=msg=>mock?mockMessage(msg):chrome.runtime.sendMessage(msg);
const report=error=>{$('message').textContent=error?.message||T.error};
const safely=fn=>async e=>{try{await fn(e)}catch(error){report(error)}};
const el=(tag,props={},...kids)=>{const node=document.createElement(tag);for(const [key,value] of Object.entries(props)){if(key==='dataset')Object.assign(node.dataset,value);else node[key]=value}node.append(...kids);if(tag==='button')decorate(node,'download');return node};
function decorate(node,name){const template=document.createElement('template');template.innerHTML=uiIcon(name);node.prepend(template.content)}
document.querySelectorAll('[data-copy]').forEach(node=>node.textContent=T[node.dataset.copy]);
for(const [id,name] of [['links','links'],['images','image'],['go','download'],['openUi','external']])decorate($(id),name);
$('filter').placeholder=T.filter;
for(const [value,text] of [['system',T.system],['light',T.light],['dark',T.dark]])$('theme').append(el('option',{value,textContent:text}));
const setTheme=value=>{localStorage.setItem('theme',value);if(value==='system')delete document.documentElement.dataset.theme;else document.documentElement.dataset.theme=value};
$('theme').value=localStorage.getItem('theme')||'system';setTheme($('theme').value);
$('theme').addEventListener('change',e=>setTheme(e.target.value));
async function showStatus(){const s=await ask({type:'status'});$('brand').textContent=s.brand;document.title=s.brand;$('capture').checked=s.capture;const st=s.status,bad=!st.ok;$('status').className=bad?'bad':'';$('status').textContent=bad?st.error==='version_mismatch'?`${T.update}: ${st.extension_version} / ${st.host_version}`:st.error==='app_not_running'?T.notRunning:/not found/i.test(st.detail||'')?T.notInstalled:`${T.notConnected} (${st.error})`:`${T.connected} · v${s.version}`}
async function showMedia(){const {media}=await ask({type:'media',tabId:tab.id});if(!media.length){$('media').className='empty';$('media').textContent=T.empty;return}$('media').className='';$('media').replaceChildren(...media.map(m=>el('div',{className:'row'},el('span',{className:'kind',textContent:m.kind}),el('span',{className:'name',title:m.url,textContent:m.name}),el('span',{className:'muted',textContent:formatSize(m.size)}),el('button',{textContent:T.download,onclick:safely(async e=>{e.target.disabled=true;try{await ask({type:'add',urls:[m.url],referer:tab.url,filename:tab.title});e.target.textContent=T.added}catch(error){e.target.disabled=false;throw error}})}))))}
const boxes=()=>[...$('list').querySelectorAll('input')];
function count(){const n=boxes().filter(b=>b.checked).length;$('count').textContent=`${n} ${T.of} ${boxes().length}`;$('go').disabled=!n}
function renderGrab(){const matches=makeFilter($('filter').value),rows=found.map((f,i)=>({f,i})).filter(({f})=>matches(`${f.url} ${f.text}`));$('list').replaceChildren(...rows.map(({f,i})=>el('label',{className:'row'},el('input',{type:'checkbox',dataset:{i},checked:false,onchange:count}),el('span',{className:'name',title:f.url,textContent:f.text||fileName(f.url)}))));if(!rows.length)$('list').append(el('p',{className:'empty',textContent:T.emptyLinks}));count()}
async function grab(mode){$('grab').hidden=false;const result=mock?[{result:previewState==='empty'?[]:[{url:'https://example.test/design-kit.zip',text:'Design resources.zip'},{url:'https://example.test/overview.pdf',text:'Project overview.pdf'},{url:'https://example.test/coast.jpg',text:'Coastal references.jpg'}]}]:await chrome.scripting.executeScript({target:{tabId:tab.id},func:collectFromPage,args:[mode]});found=result[0]?.result||[];renderGrab()}
$('capture').onchange=safely(e=>ask({type:'set_capture',on:e.target.checked}));
$('links').onclick=safely(()=>grab('links'));$('images').onclick=safely(()=>grab('images'));$('filter').oninput=renderGrab;
$('all').onclick=()=>{boxes().forEach(b=>b.checked=true);count()};$('none').onclick=()=>{boxes().forEach(b=>b.checked=false);count()};$('openUi').onclick=safely(()=>ask({type:'open_ui'}));
$('go').onclick=safely(async()=>{const urls=boxes().filter(b=>b.checked).map(b=>found[b.dataset.i].url);$('go').disabled=true;try{await ask({type:'add',urls,referer:tab.url});$('count').textContent=`${urls.length} ${T.added.toLowerCase()}`}catch(error){count();throw error}});
async function mockMessage(msg){if(msg.type==='status'){const {BRAND_NAME}=await import('./brand.js');return{brand:BRAND_NAME,capture:true,version:'0.1.0',status:['connected','empty'].includes(previewState)?{ok:true}:{ok:false,error:previewState,detail:previewState==='missing'?'not found':'',extension_version:'0.1.0',host_version:'0.0.9'}}}if(msg.type==='media')return{media:previewState==='empty'?[]:[{kind:'hls',name:'Coastal walk · 4K',url:'https://example.test/coast.m3u8',size:347000000},{kind:'audio',name:'Interview recording',url:'https://example.test/audio.mp3',size:24800000}]};return{ok:true}}
await safely(async()=>{if(mock){tab={id:1,url:'https://example.test',title:'Coastal walk'};$('preview').hidden=false;for(const [value,text] of [['connected',T.connected],['empty',T.emptyLinks],['app_not_running',T.notRunning],['missing',T.notInstalled],['version_mismatch',T.update]])$('preview-state').append(el('option',{value,textContent:text}));$('preview-state').value=previewState;$('preview-state').onchange=safely(async e=>{previewState=e.target.value;await showStatus();await showMedia();await grab('links')});if(new URLSearchParams(location.search).get('dir')==='rtl')document.documentElement.dir='rtl'}else [tab]=await chrome.tabs.query({active:true,currentWindow:true});await showStatus();if(tab)await showMedia();if(mock)await grab('links');else{const {grab:want}=await chrome.storage.session.get('grab');if(want){await chrome.storage.session.remove('grab');await grab(want)}}})();
