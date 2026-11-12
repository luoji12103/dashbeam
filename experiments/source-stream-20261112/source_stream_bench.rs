//! Linux DEV-only engine/data-plane A/B; no app/UI or Android SAF orchestration.
use anyhow::{ensure, Context, Result};
use engine::{AppHandle, DiscoveryModeOption, EventEmitter, ReceiveOptions, RelayModeOption, SendOptions};
use native::stream::{StreamReceiveOptions, StreamReceiver, StreamShare, StreamTicket};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet}, fs::{self, File, OpenOptions}, io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt}, path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, time::{Duration, Instant},
};

fn ns() -> u64 {
    let mut ts = nix::libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { nix::libc::clock_gettime(nix::libc::CLOCK_MONOTONIC, &mut ts) }, 0);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
fn process() -> Value {
    let io: BTreeMap<String, u64> = fs::read_to_string("/proc/self/io").unwrap_or_default().lines()
        .filter_map(|l| { let (k,v)=l.split_once(':')?; Some((k.to_owned(),v.trim().parse().ok()?)) }).collect();
    let mut u = unsafe { std::mem::zeroed::<nix::libc::rusage>() };
    assert_eq!(unsafe { nix::libc::getrusage(nix::libc::RUSAGE_SELF, &mut u) },0);
    json!({"io":io,"maxrss_kib":u.ru_maxrss,
        "cpu_user_us":u.ru_utime.tv_sec*1_000_000+u.ru_utime.tv_usec,
        "cpu_system_us":u.ru_stime.tv_sec*1_000_000+u.ru_stime.tv_usec})
}
fn disk(path: &Path, seen: &mut HashSet<(u64,u64)>) -> (u64,u64) {
    let Ok(m)=fs::symlink_metadata(path) else { return (0,0) };
    if m.is_file() { return if seen.insert((m.dev(),m.ino())) {(m.len(),m.blocks()*512)}else{(0,0)}; }
    if !m.is_dir() { return (0,0); }
    fs::read_dir(path).into_iter().flatten().flatten().map(|e|disk(&e.path(),seen))
        .fold((0,0),|(a,b),(c,d)|(a+c,b+d))
}
struct Sampler { stop: Arc<AtomicBool>, worker: Option<std::thread::JoinHandle<(u64,u64)>> }
impl Sampler {
    fn start(root: PathBuf) -> Self {
        let stop=Arc::new(AtomicBool::new(false)); let flag=stop.clone();
        let worker=std::thread::spawn(move|| {
            let mut peak=(0,0);
            loop { let d=disk(&root,&mut HashSet::new()); peak.0=peak.0.max(d.0); peak.1=peak.1.max(d.1);
                if flag.load(Ordering::Relaxed) { break; } std::thread::sleep(Duration::from_millis(10)); }
            peak
        }); Self {stop,worker:Some(worker)}
    }
    fn finish(mut self)->Value { self.stop.store(true,Ordering::Relaxed);
        let (logical,allocated)=self.worker.take().unwrap().join().unwrap();
        json!({"peak_logical_bytes":logical,"peak_allocated_bytes":allocated,"sample_interval_ms":10}) }
}
impl Drop for Sampler {fn drop(&mut self){ self.stop.store(true,Ordering::Relaxed); if let Some(w)=self.worker.take(){let _=w.join();}}}

#[derive(Default)] struct Recorder(Mutex<BTreeMap<String,u64>>);
impl EventEmitter for Recorder {
    fn emit_event(&self,name:&str)->std::result::Result<(),String>{
        self.0.lock().unwrap().entry(name.to_owned()).or_insert_with(ns); Ok(()) }
    fn emit_event_with_payload(&self,name:&str,_:&str)->std::result::Result<(),String>{self.emit_event(name)}
}
fn atomic(path:&Path,content:&[u8])->Result<()> {
    let tmp=path.with_extension("tmp"); let mut f=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all(content)?; f.sync_all()?; fs::rename(tmp,path)?; Ok(())
}
async fn wait(path:&Path)->Result<()> {
    let end=Instant::now()+Duration::from_secs(240);
    while !path.exists(){ensure!(!path.parent().context("no parent")?.join("failed").exists(),"peer benchmark failed");ensure!(Instant::now()<end,"coordination timeout"); tokio::time::sleep(Duration::from_millis(10)).await;} Ok(())
}
fn compare(source:&Path,dest:&Path)->Result<()> {
    ensure!(fs::metadata(source)?.len()==fs::metadata(dest)?.len(),"byte length differs");
    let (mut a,mut b)=(File::open(source)?,File::open(dest)?); let (mut x,mut y)=([0u8;65536],[0u8;65536]);
    loop{let n=a.read(&mut x)?; if n==0{break;} b.read_exact(&mut y[..n])?;ensure!(x[..n]==y[..n],"file bytes differ");} Ok(())
}
struct Args { role:String,variant:String,shared:PathBuf,source:PathBuf,relay:String,count:usize,window_bytes:u32 }
fn args()->Result<Args>{
    let mut m=BTreeMap::new();let mut it=std::env::args().skip(1);
    while let Some(k)=it.next(){ensure!(k.starts_with("--"),"bad option");ensure!(m.insert(k,it.next().context("missing value")?).is_none(),"duplicate option");}
    ensure!((6..=7).contains(&m.len())&&m.keys().all(|k|["--role","--variant","--shared","--source","--relay","--count","--window-mib"].contains(&k.as_str())),"unexpected benchmark options");
    let get=|k:&str|->Result<String>{m.get(k).cloned().context("missing option")};
    let window_bytes=if let Some(window)=m.get("--window-mib") {let mib:u32=window.parse()?;ensure!((1..=16).contains(&mib),"invalid benchmark window");mib*1024*1024}else{native::stream::StreamLimits::default().transport_window_bytes};
    let a=Args{role:get("--role")?,variant:get("--variant")?,shared:get("--shared")?.into(),source:get("--source")?.into(),relay:get("--relay")?,count:get("--count")?.parse()?,window_bytes};
    ensure!(["send","recv"].contains(&a.role.as_str())&&["legacy","stream"].contains(&a.variant.as_str()),"bad role/variant");
    ensure!((1..=3).contains(&a.count),"bad count"); Ok(a)
}
fn relay(a:&Args)->Result<RelayModeOption>{ Ok(if a.relay=="disabled"{RelayModeOption::Disabled}else{
    ensure!(a.relay=="https://relay.dashbeam.test:3341","unknown relay");RelayModeOption::Custom{urls:vec![a.relay.parse()?],auth_token:None}}) }
fn discovery()->Result<DiscoveryModeOption>{Ok(DiscoveryModeOption::Custom{pkarr_relay_url:"https://relay.dashbeam.test:3341/pkarr".parse()?,dns_origin:None})}
fn send_options(a:&Args)->Result<SendOptions>{Ok(SendOptions{relay_mode:relay(a)?,discovery_mode:discovery()?,ticket_type:engine::AddrInfoOptions::RelayAndAddresses,..Default::default()})}
fn recv_options(a:&Args,out:PathBuf)->Result<ReceiveOptions>{Ok(ReceiveOptions{relay_mode:relay(a)?,discovery_mode:discovery()?,output_dir:Some(out),..Default::default()})}
fn stream_limits(a:&Args)->native::stream::StreamLimits{
    native::stream::StreamLimits{transport_window_bytes:a.window_bytes,..Default::default()}
}
fn stream_window_bytes(a:&Args)->Option<u32>{
    (a.variant=="stream").then_some(a.window_bytes)
}

async fn send(a:&Args)->Result<()> {
    let temp=a.shared.join("sender-temp"); fs::create_dir(&temp)?; native::storage::TEMP_DIR.set(temp.clone()).unwrap();
    let sample=Sampler::start(temp); let io_before=process(); let intent=ns();
    let (ticket,legacy,stream)=if a.variant=="legacy" {
        let s=engine::start_share_items(vec![a.source.clone()],send_options(a)?,&None,None).await?;
        (s.ticket.clone(),Some(s),None)
    }else{let s=StreamShare::start_with_limits(a.source.clone(),send_options(a)?,stream_limits(a)).await?;(s.ticket().to_token()?,None,Some(s))};
    let ready=ns(); let prepare_io=process();
    let before_read=stream.as_ref().map(|s|s.source_bytes_read());
    atomic(&a.shared.join("ticket"),ticket.as_bytes())?;
    wait(&a.shared.join("done")).await?;
    let data_done=ns(); let io_after=process(); let source_read=stream.as_ref().map(|s|s.source_bytes_read());
    if let Some(s)=legacy{ s.router.shutdown().await?;s.router.endpoint().close().await;drop(s); }
    if let Some(s)=stream{s.shutdown().await;}
    let result=json!({"share_intent_ns":intent,"ticket_ready_ns":ready,"data_done_ns":data_done,
        "before":io_before,"prepared":prepare_io,"after":io_after,"source_bytes_read_at_ready":before_read,
        "source_bytes_read_final":source_read,
        "stream_transport_window_bytes":stream_window_bytes(a),"disk":sample.finish(),"shutdown_ns":ns()});
    atomic(&a.shared.join("sender.json"),&serde_json::to_vec_pretty(&result)?)?;Ok(())
}
async fn receive(a:&Args)->Result<()> {
    wait(&a.shared.join("ticket")).await?; let token=fs::read_to_string(a.shared.join("ticket"))?;
    let temp=a.shared.join("receiver-temp");fs::create_dir(&temp)?;native::storage::TEMP_DIR.set(temp.clone()).unwrap();
    let output=a.shared.join("receiver-output"); fs::create_dir(&output)?;
    fs::set_permissions(&output,fs::Permissions::from_mode(0o700))?;
    // Monitor includes store AND final output. The two stores are never conflated with source fixtures.
    let sample=Sampler::start(a.shared.clone());
    let initial_io=process(); let receiver_intent=ns();
    let mut stream_client=if a.variant=="stream"{Some(StreamReceiver::new(StreamReceiveOptions{network:recv_options(a,output.clone())?,limits:stream_limits(a)}).await?)}else{None};
    let parsed=if a.variant=="stream"{Some(StreamTicket::from_token(&token)?)}else{None};
    let mut transfers=Vec::new();
    for i in 0..a.count{
        let out=output.join(i.to_string());fs::create_dir(&out)?;
        fs::set_permissions(&out,fs::Permissions::from_mode(0o700))?;
        let start=ns();let io_before=process();let rec=Arc::new(Recorder::default());let emitter:AppHandle=Some(rec.clone());
        engine::benchmark_set_observer(emitter.clone());
        let (_tx,rx)=tokio::sync::oneshot::channel();
        let (path,first,initial_path,final_path,ack,committed)=if let Some(c)=stream_client.as_mut(){
            let call_start=ns();
            let r=c.receive(parsed.as_ref().unwrap(),out.clone(),rx).await?;
            (r.path,r.first_payload_elapsed.map(|d|call_start+(d.as_nanos() as u64)),format!("{:?}",r.initial_path).to_lowercase(),format!("{:?}",r.final_path).to_lowercase(),Some(r.completion_ack_sent),Some(call_start+r.committed_elapsed.as_nanos() as u64))
        }else{
            let r=engine::download(token.clone(),recv_options(a,out.clone())?,emitter,rx).await?;
            let e=rec.0.lock().unwrap();let p=|prefix:&str|e.keys().find_map(|k|k.strip_prefix(prefix).map(str::to_string)).unwrap_or_else(||"unknown".into());
            (r.exported_files.first().context("no exported file")?.path.clone(),e.get("benchmark-first-payload-progress").copied(),p("benchmark-initial-path-"),p("benchmark-final-path-"),None,e.get("receive-completed").copied())
        };
        let returned=ns();let io_after=process();compare(&a.source,&path)?;
        transfers.push(json!({"index":i,"receive_start_ns":start,"receive_return_ns":returned,
            "first_payload_ns":first,"committed_ns":committed,"initial_path":initial_path,"final_path":final_path,"completion_ack_sent":ack,
            "bytes_match":true,"bytes":fs::metadata(&path)?.len(),"before":io_before,"after":io_after}));
        // Only this sample's synthetic output, after independently validating its bytes.
        fs::remove_dir_all(out)?;
    }
    let after_io=process();if let Some(c)=stream_client{c.shutdown().await;}
    let result=json!({"receiver_intent_ns":receiver_intent,"transfers":transfers,"before":initial_io,"after":after_io,
        "disk":sample.finish(),"shutdown_ns":ns(),"bytes_match":true,"stream_transport_window_bytes":stream_window_bytes(a)});
    atomic(&a.shared.join("receiver.json"),&serde_json::to_vec_pretty(&result)?)?;
    atomic(&a.shared.join("done"),b"ok")?;Ok(())
}
#[tokio::main(flavor="multi_thread",worker_threads=2)]
async fn main(){
    let a=match args(){Ok(a)=>a,Err(e)=>{eprintln!("invalid benchmark arguments: {e}");std::process::exit(1);}};
    let result=match a.role.as_str(){"send"=>send(&a).await,"recv"=>receive(&a).await,_=>unreachable!("validated role")};
    if result.is_err()&&a.role=="recv"{let _=atomic(&a.shared.join("failed"),b"receiver failed");}
    if let Err(e)=result{eprintln!("engine benchmark failed: {e:#}");std::process::exit(1);}
}
