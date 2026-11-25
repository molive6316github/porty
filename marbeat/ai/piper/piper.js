// Local neural TTS (Piper VITS via onnxruntime-web). Everything loads from this folder: no CDN.
// Model: en_GB-vctk-medium (109 speakers). Phonemizer: espeak-ng compiled to wasm (piper_phonemize).
function loadScript(src){
  return new Promise((res,rej)=>{
    if(window.ort)return res();
    const s=document.createElement("script");s.src=src;s.onload=()=>res();s.onerror=()=>rej(new Error("could not load "+src));
    document.head.appendChild(s);
  });
}
async function fetchWithProgress(url,onProgress){
  const r=await fetch(url);if(!r.ok)throw new Error(`could not load ${url} (${r.status})`);
  const total=+r.headers.get("Content-Length")||0,reader=r.body.getReader(),parts=[];let got=0;
  for(;;){const {done,value}=await reader.read();if(done)break;parts.push(value);got+=value.length;if(onProgress)onProgress(got,total)}
  const out=new Uint8Array(got);let o=0;for(const p of parts){out.set(p,o);o+=p.length}return out.buffer;
}
export async function createPiper(base,onProgress){
  await loadScript(base+"ort.wasm.min.js");
  const ort=window.ort;
  ort.env.wasm.wasmPaths=base;ort.env.wasm.numThreads=1;
  const cfg=await (await fetch(base+"en_GB-vctk-medium.onnx.json")).json();
  const model=await fetchWithProgress(base+"en_GB-vctk-medium.onnx",onProgress);
  const session=await ort.InferenceSession.create(model,{executionProviders:["wasm"]});
  const pp=await import(base+"piper-phonemize.mjs");

  // words -> phoneme id arrays (one wasm run for the whole list)
  async function phonemize(words){
    const lines=[];
    const m=await pp.createPiperPhonemize({
      print:l=>{try{lines.push(JSON.parse(l).phoneme_ids)}catch(e){}},
      printErr:()=>{},
      locateFile:f=>f.endsWith(".wasm")?base+"piper_phonemize.wasm":f.endsWith(".data")?base+"piper_phonemize.data":f
    });
    m.callMain(["-l",cfg.espeak.voice,"--input",JSON.stringify(words.map(w=>({text:w}))),"--espeak_data","/espeak-ng-data"]);
    return lines;
  }
  async function synth(ids,speaker,lengthScale,noiseScale,noiseW){
    const inf=cfg.inference;
    const feeds={
      input:new ort.Tensor("int64",BigInt64Array.from(ids,x=>BigInt(x)),[1,ids.length]),
      input_lengths:new ort.Tensor("int64",BigInt64Array.from([BigInt(ids.length)]),[1]),
      scales:new ort.Tensor("float32",Float32Array.from([noiseScale!==undefined?noiseScale:inf.noise_scale,lengthScale||inf.length_scale,noiseW!==undefined?noiseW:inf.noise_w]),[3]),
      sid:new ort.Tensor("int64",BigInt64Array.from([BigInt(speaker)]),[1])
    };
    const r=await session.run(feeds);
    return new Float32Array(r.output.data);
  }
  return{phonemize,synth,sampleRate:cfg.audio.sample_rate,speakers:Object.keys(cfg.speaker_id_map).length,config:cfg};
}
