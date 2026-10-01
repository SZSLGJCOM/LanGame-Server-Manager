#[path = "runtime_build.rs"]
mod runtime_build;

fn main() {
    if let Err(error) = runtime_build::prepare() {
        panic!("Cannot prepare the bundled ONNX CPU runtime: {error}");
    }
}
