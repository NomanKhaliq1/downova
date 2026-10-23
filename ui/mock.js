import { brand } from './brand.js';
import { expandPreview } from './helpers.js';
const categories = ['Compressed','Documents','Music','Video','Images','Programs','Other'];
export const mockSettings = () => ({download_root:'C:\\Preview\\Downloads',categories:categories.map((name,i)=>({name,extensions:[['zip','7z'],['pdf','docx'],['mp3','flac'],['mp4','mkv'],['jpg','png'],['exe','msi'],[]][i],folder:name})),other_folder:'Other',site_rules:[],exclusions:{extensions:['torrent'],sites:[]},connections:8,max_simultaneous:3,speed_limit:null,user_agent:null,proxy:null,on_complete_command:null,clipboard_monitor:true,start_with_windows:false,ffmpeg_path:null});
export function mockTasks(count=8) {
  const names=['Design resources.zip','Coastal walk · 4K.mp4','Windows installer.exe','Project overview.pdf','Night drive.flac','Brand references.zip','Summer collection.jpg','Interview recording.mp4'];
  const statuses=['Downloading','Downloading','Queued','Done','Paused','Done','Failed','Queued'];
  return Array.from({length:count},(_,i)=>({id:i+1,url:`https://example.test/files/${i+1}`,name:names[i%8].replace('.',i>7?` ${i+1}.`:'.'),path:`C:\\Preview\\Downloads\\${names[i%8]}`,dir:'C:\\Preview\\Downloads',category:['Compressed','Video','Programs','Documents','Music','Compressed','Images','Video'][i%8],queue:i%3?'Main':'Night',status:statuses[i%8],total:(i%8+1)*158334976,downloaded:statuses[i%8]==='Done'?(i%8+1)*158334976:Math.round((i%8+1)*158334976*.37),speed:statuses[i%8]==='Downloading'?5242880+i*100000:0,eta_secs:statuses[i%8]==='Downloading'?28+i*15:null,error:statuses[i%8]==='Failed'?'HTTP 403 · Access denied':null,note:i===1?'Separate video and audio will be merged with ffmpeg.':'',added:1792780170-i*700,finished:null,priority:0,opts:{connections:8,headers:[],user_agent:null}}));
}
export function createMock() {
  let tasks=mockTasks(), settings=mockSettings(), queues=[{name:'Main',enabled:true,max_simultaneous:3,schedule:null,on_finish:'None'},{name:'Night',enabled:true,max_simultaneous:2,schedule:{days:127,start:1380,stop:360},on_finish:'None'}];
  return {setTasks(value){tasks=value},tick(){tasks.filter(t=>t.status==='Downloading').forEach(t=>{t.downloaded=Math.min(t.total,t.downloaded+t.speed/2);t.eta_secs=Math.ceil((t.total-t.downloaded)/t.speed);if(t.downloaded===t.total){t.status='Done';t.speed=0}});return structuredClone(tasks)},async request(path,method='GET',body){
    const [route,query='']=path.split('?');const params=new URLSearchParams(query);const parts=route.split('/').filter(Boolean);let data={ok:true};
    if(route==='/ping') data={app:brand.name,version:'0.1.0'};
    else if(route==='/settings'){if(method==='PUT')settings=structuredClone(body);data=settings;}
    else if(route==='/queues')data=queues;
    else if(parts[0]==='queues'){const name=decodeURIComponent(parts[1]);if(method==='DELETE'){queues=queues.filter(q=>q.name!==name);tasks.forEach(t=>{if(t.queue===name)t.queue='Main'})}else{queues=queues.filter(q=>q.name!==name);queues.push({...body,name});}}
    else if(route==='/export') data=tasks.filter(t=>(!params.get('status')||t.status===params.get('status'))&&(!params.get('queue')||t.queue===params.get('queue'))).map(t=>t.url).join('\n');
    else if(route==='/tasks' && method==='POST'){data={added:[],duplicates:[],invalid:[]};for(const url of expandPreview(body.urls)){if(!/^https?:\/\//i.test(url)){data.invalid.push(url);continue}const duplicate=tasks.find(t=>t.url===url);if(duplicate){data.duplicates.push(duplicate.id);continue}const id=Math.max(0,...tasks.map(t=>t.id))+1;tasks.unshift({...mockTasks(1)[0],...body,id,url,name:body.filename||url.split('/').pop(),status:body.paused?'Paused':'Queued',downloaded:0,speed:0});data.added.push(id)}}
    else if(route==='/tasks')data=tasks.filter(t=>(!params.get('q')||`${t.name} ${t.url}`.toLowerCase().includes(params.get('q').toLowerCase())));
    else if(route==='/tasks/pause-all'||route==='/tasks/resume-all'){tasks.forEach(t=>{if(['Downloading','Paused','Queued'].includes(t.status))t.status=route.endsWith('pause-all')?'Paused':'Queued'})}
    else if(route==='/tasks/reorder'){tasks.sort((a,b)=>body.ids.indexOf(a.id)-body.ids.indexOf(b.id))}
    else if(parts[0]==='tasks'){const t=tasks.find(t=>t.id===+parts[1]);if(!t)return{ok:false,data:{error:'Task not found'}};if(method==='DELETE')tasks=tasks.filter(x=>x!==t);else if(parts[2]==='pause')t.status='Paused';else if(parts[2]==='resume')t.status='Queued';else if(parts[2]==='queue')t.queue=body.queue;else data=t;}
    return{ok:true,data:structuredClone(data)};
  }};
}
