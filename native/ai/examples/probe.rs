use ort::session::Session;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    for m in &a[2..] {
        match Session::builder().unwrap().commit_from_file(m) {
            Ok(s) => println!("{m}: OK in={:?} out={:?}", s.inputs.iter().map(|i| (&i.name, format!("{:?}", i.input_type))).collect::<Vec<_>>(), s.outputs.iter().map(|o| &o.name).collect::<Vec<_>>()),
            Err(e) => println!("{m}: FAIL {e}"),
        }
    }
}
