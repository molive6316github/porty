// Marbeat vocal separation worker: Spleeter (Deezer, MIT) 2-stem models in the browser.
// stereo 44.1 kHz audio -> STFT (4096/1024) -> model estimates the vocal magnitude -> soft mask -> inverse STFT.
// vocals = masked mix; instrumental = mix - vocals, so the two always add back up to the original.
// ONNX export: csukuangfj/sherpa-onnx-spleeter-2stems.
let ort=null,mV=null;
const N=4096,H=1024,NB=N/2+1,FB=1024,T=512;
const ortBase=new URL("../clone/",self.location.href).href,here=new URL("./",self.location.href).href;
const say=text=>postMessage({type:"status",text});
const win=new Float32Array(N);for(let i=0;i<N;i++)win[i]=.5-.5*Math.cos(2*Math.PI*i/N); // periodic Hann
// radix-2 FFT (in place), twiddles cached
const cosT=new Float32Array(N/2),sinT=new Float32Array(N/2);for(let i=0;i<N/2;i++){cosT[i]=Math.cos(2*Math.PI*i/N);sinT[i]=-Math.sin(2*Math.PI*i/N)}
const rev=new Uint16Array(N);for(let i=0,b=Math.log2(N);i<N;i++){let r=0;for(let k=0;k<b;k++)if(i&(1<<k))r|=1<<(b-1-k);rev[i]=r}
function fft(re,im){
  for(let i=0;i<N;i++){const j=rev[i];if(j>i){let t=re[i];re[i]=re[j];re[j]=t;t=im[i];im[i]=im[j];im[j]=t}}
  for(let size=2;size<=N;size<<=1){
    const half=size>>1,step=N/size;
    for(let s=0;s<N;s+=size){
      for(let k=0,t=0;k<half;k++,t+=step){
        const a=s+k,b=a+half,wr=cosT[t],wi=sinT[t];
        const xr=re[b]*wr-im[b]*wi,xi=re[b]*wi+im[b]*wr;
        re[b]=re[a]-xr;im[b]=im[a]-xi;re[a]+=xr;im[a]+=xi;
      }
    }
  }
}
async function fetchBuf(url,label){
  const r=await fetch(url);if(!r.ok)throw new Error(`could not load ${url} (${r.status})`);
  const total=+r.headers.get("Content-Length")||0,reader=r.body.getReader(),parts=[];let got=0;
  for(;;){const {done,value}=await reader.read();if(done)break;parts.push(value);got+=value.length;say(`Loading the vocal separator ${label}… ${total?Math.round(got/total*100)+"%":(got/1e6).toFixed(0)+" MB"}`)}
  const out=new Uint8Array(got);let o=0;for(const p of parts){out.set(p,o);o+=p.length}return out.buffer;
}
async function load(){
  const m=await import(ortBase+"ort.min.mjs");ort=m.default||m;ort.env.wasm.wasmPaths=ortBase;ort.env.wasm.numThreads=1;
  const o={executionProviders:["wasm"],graphOptimizationLevel:"all"};
  mV=await ort.InferenceSession.create(await fetchBuf(here+"vocals.onnx",""),o);
}
// complex STFT of one channel: frames start at 0, zero-padded at the end
function stft(x,frames){
  const re=new Float32Array(frames*NB),im=new Float32Array(frames*NB),fr=new Float32Array(N),fi=new Float32Array(N);
  for(let t=0;t<frames;t++){
    const o=t*H;for(let i=0;i<N;i++){const v=o+i<x.length?x[o+i]:0;fr[i]=v*win[i];fi[i]=0}
    fft(fr,fi);re.set(fr.subarray(0,NB),t*NB);im.set(fi.subarray(0,NB),t*NB);
  }
  return{re,im};
}
function istft(re,im,frames,len){
  const out=new Float32Array(frames*H+N),wsum=new Float32Array(frames*H+N),fr=new Float32Array(N),fi=new Float32Array(N);
  for(let t=0;t<frames;t++){
    const b=t*NB;
    for(let k=0;k<NB;k++){fr[k]=re[b+k];fi[k]=-im[b+k]}              // conjugate trick for the inverse transform
    for(let k=1;k<N/2;k++){fr[N-k]=re[b+k];fi[N-k]=im[b+k]}          // Hermitian mirror (already conjugated)
    fft(fr,fi);const o=t*H;
    for(let i=0;i<N;i++){out[o+i]+=fr[i]/N*win[i];wsum[o+i]+=win[i]*win[i]}
  }
  const y=new Float32Array(len);for(let i=0;i<len;i++)y[i]=wsum[i]>1e-3?out[i]/wsum[i]:0;
  return y;
}
async function separate(L,R,mode){
  const len=L.length,frames=Math.ceil(len/H),S=Math.ceil(frames/T);
  const t0=performance.now();
  const spec=[stft(L,frames),stft(R,frames)];
  // model input [2, S, 512, 1024]: magnitudes of the lowest 1024 bins
  const x=new Float32Array(2*S*T*FB);
  for(let c=0;c<2;c++)for(let t=0;t<frames;t++){
    const s=Math.floor(t/T),f=t%T,o=((c*S+s)*T+f)*FB,b=t*NB,sr_=spec[c].re,si=spec[c].im;
    for(let k=0;k<FB;k++)x[o+k]=Math.hypot(sr_[b+k],si[b+k]);
  }
  const t1=performance.now();
  const inp=()=>new ort.Tensor("float32",x,[2,S,T,FB]);
  say("Separating the vocals from the music…");
  const yv=(await mV.run({x:inp()})).y.data;
  const ya=null; // single-model mode: accompaniment is estimated as (mix - vocals); nearly as good as using both models
  const t2=performance.now();
  const eps=1e-10,vocL=[],outV=[];
  for(let c=0;c<2;c++){
    const re=new Float32Array(frames*NB),im=new Float32Array(frames*NB);
    for(let t=0;t<frames;t++){
      const s=Math.floor(t/T),f=t%T,o=((c*S+s)*T+f)*FB,b=t*NB;
      for(let k=0;k<FB;k++){
        const v=yv[o+k];let a;
        if(ya)a=ya[o+k];else a=Math.max(0,x[o+k]-v); // fast mode: accompaniment ~ mix - vocals
        const m=(v*v+eps/2)/(v*v+a*a+eps);
        re[b+k]=spec[c].re[b+k]*m;im[b+k]=spec[c].im[b+k]*m;
      }
    }
    outV.push(istft(re,im,frames,len));
  }
  const t3=performance.now();
  return{vocL:outV[0],vocR:outV[1],ms:{stft:Math.round(t1-t0),model:Math.round(t2-t1),mask_istft:Math.round(t3-t2)},frames};
}
self.onmessage=async e=>{
  const {type,id,left,right,mode}=e.data;
  try{
    if(type==="load"){await load();postMessage({type:"loaded"});return}
    if(type==="separate"){
      const r=await separate(left,right,"fast");
      postMessage({type:"separated",id,vocL:r.vocL,vocR:r.vocR,ms:r.ms,frames:r.frames},[r.vocL.buffer,r.vocR.buffer]);return;
    }
  }catch(err){postMessage({type:"error",id,error:String(err&&err.message||err)})}
};
