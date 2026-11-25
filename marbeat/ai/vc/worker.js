// Marbeat voice changer worker: kNN-VC (Baas et al., MIT) in the browser.
// source audio -> WavLM-Large layer-6 features -> replace every frame by the mean of its k nearest frames
// from the target voice's sample -> HiFi-GAN vocoder -> audio in the target voice. 16 kHz mono throughout.
// Models: TigreGotico/voiceclonnx-knn-vc (MIT). WavLM was re-quantized (matrix layers only) so it runs in onnxruntime-web.
let ort=null,wavlm=null,hifi=null,busy=false;
const refs=new Map(); // name -> {raw:Float32Array(M*D), unit:Float32Array(M*D), M}
const D=1024,K=4;
const here=new URL("./",self.location.href).href;
const ortBase=new URL("../clone/",self.location.href).href; // onnxruntime-web files are shared with the cloning engine
const say=(text)=>postMessage({type:"status",text});

async function fetchAll(urls,label){
  const bufs=[];let got=0,total=0;
  const heads=await Promise.all(urls.map(u=>fetch(u,{method:"HEAD"}).then(r=>+r.headers.get("Content-Length")||0).catch(()=>0)));
  total=heads.reduce((a,b)=>a+b,0);
  for(const u of urls){
    const r=await fetch(u);if(!r.ok)throw new Error(`could not load ${u} (${r.status})`);
    const reader=r.body.getReader();
    for(;;){const {done,value}=await reader.read();if(done)break;bufs.push(value);got+=value.length;
      say(`Loading voice changer ${label}… ${total?Math.round(got/total*100)+"%":(got/1e6).toFixed(0)+" MB"}`)}
  }
  const out=new Uint8Array(got);let o=0;for(const b of bufs){out.set(b,o);o+=b.length}
  return out.buffer;
}
async function load(){
  const m=await import(ortBase+"ort.min.mjs");ort=m.default||m;
  ort.env.wasm.wasmPaths=ortBase;ort.env.wasm.numThreads=1;
  const wl=await fetchAll([here+"wavlm_mm8.part00",here+"wavlm_mm8.part01"],"(1/2)");
  say("Starting the voice changer (feature model)…");
  wavlm=await ort.InferenceSession.create(wl,{executionProviders:["wasm"],graphOptimizationLevel:"all"});
  const hf=await fetchAll([here+"hifigan_knnvc.onnx"],"(2/2)");
  say("Starting the voice changer (vocoder)…");
  hifi=await ort.InferenceSession.create(hf,{executionProviders:["wasm"],graphOptimizationLevel:"all"});
}
// 16 kHz audio -> [T x 1024] features (50 Hz). Long audio is processed in ~10 s pieces.
async function features(audio){
  const CH=160000,parts=[];let T=0;
  for(let o=0;o<audio.length;o+=CH){
    const seg=audio.subarray(o,Math.min(audio.length,o+CH));if(seg.length<1600)continue;
    // WavLM-Large expects zero-mean, unit-variance input
    let mean=0;for(let i=0;i<seg.length;i++)mean+=seg[i];mean/=seg.length;
    let v=0;for(let i=0;i<seg.length;i++){const d=seg[i]-mean;v+=d*d}const sd=Math.sqrt(v/seg.length+1e-7);
    const x=new Float32Array(seg.length);for(let i=0;i<seg.length;i++)x[i]=(seg[i]-mean)/sd;
    const r=await wavlm.run({input_values:new ort.Tensor("float32",x,[1,x.length])});
    const h=r.hidden_states;parts.push(new Float32Array(h.data));T+=h.dims[1];
    await new Promise(r=>setTimeout(r,0));
  }
  const out=new Float32Array(T*D);let o=0;for(const p of parts){out.set(p,o);o+=p.length}
  return{data:out,T};
}
function unitRows(raw,M){
  const u=new Float32Array(raw.length);
  for(let m=0;m<M;m++){let n=0;const b=m*D;for(let d=0;d<D;d++)n+=raw[b+d]*raw[b+d];n=1/(Math.sqrt(n)+1e-8);for(let d=0;d<D;d++)u[b+d]=raw[b+d]*n}
  return u;
}
// kNN regression: each source frame -> mean of its K most cosine-similar frames of the target voice
function knn(src,T,ref){
  const out=new Float32Array(T*D),{raw,unit,M}=ref;
  const bestS=new Float32Array(K),bestI=new Int32Array(K);
  for(let t=0;t<T;t++){
    const b=t*D;let n=0;for(let d=0;d<D;d++)n+=src[b+d]*src[b+d];n=1/(Math.sqrt(n)+1e-8);
    bestS.fill(-2);bestI.fill(0);
    for(let m=0;m<M;m++){
      const mb=m*D;let dot=0;for(let d=0;d<D;d++)dot+=src[b+d]*unit[mb+d];dot*=n;
      if(dot>bestS[K-1]){ // insert into the sorted top-K
        let j=K-1;while(j>0&&bestS[j-1]<dot){bestS[j]=bestS[j-1];bestI[j]=bestI[j-1];j--}
        bestS[j]=dot;bestI[j]=m;
      }
    }
    for(let k=0;k<K;k++){const mb=bestI[k]*D;for(let d=0;d<D;d++)out[b+d]+=raw[mb+d]/K}
  }
  return out;
}
async function vocode(feats,T){
  const x=new Float32Array(D*T);                      // [1, 1024, T] (channels first)
  for(let t=0;t<T;t++)for(let d=0;d<D;d++)x[d*T+t]=feats[t*D+d];
  const r=await hifi.run({features:new ort.Tensor("float32",x,[1,D,T])});
  // this vocoder emits 256 samples per 20 ms feature frame (the frame is 320 samples at 16 kHz), so stretch by 1.25
  // to restore the true timing and pitch of the converted voice
  const w=r.waveform.data,ratio=320/256,N=Math.floor(w.length*ratio),y=new Float32Array(N);
  for(let j=0;j<N;j++){
    const p=j/ratio,i=Math.floor(p),f=p-i;
    const a=w[Math.max(0,i-1)],b=w[i],c=w[Math.min(w.length-1,i+1)],d=w[Math.min(w.length-1,i+2)];
    y[j]=b+.5*f*(c-a+f*(2*a-5*b+4*c-d+f*(3*(b-c)+d-a))); // cubic interpolation
  }
  return y;
}

self.onmessage=async(e)=>{
  const {type,id,name,audio}=e.data;
  try{
    if(type==="load"){await load();postMessage({type:"loaded"});return}
    if(!wavlm||!hifi)throw new Error("voice changer not loaded");
    if(type==="setref"){
      say("Learning the target voice…");
      const f=await features(audio);
      refs.set(name,{raw:f.data,unit:unitRows(f.data,f.T),M:f.T});
      postMessage({type:"refset",name,frames:f.T});return;
    }
    if(type==="convert"){
      const ref=refs.get(name);if(!ref)throw new Error("no reference voice named "+name);
      const t0=performance.now(),f=await features(audio),t1=performance.now();
      const m=knn(f.data,f.T,ref),t2=performance.now();
      const wav=await vocode(m,f.T),t3=performance.now();
      postMessage({type:"converted",id,audio:wav,ms:{wavlm:Math.round(t1-t0),knn:Math.round(t2-t1),vocoder:Math.round(t3-t2)},frames:f.T},[wav.buffer]);return;
    }
  }catch(err){postMessage({type:"error",id,error:String(err&&err.message||err)})}
};
