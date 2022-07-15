use futures::channel::mpsc::{Receiver, Sender};
use wasm_bindgen::prelude::wasm_bindgen;
use alloc::string::String;

struct Chat {
    reciver: Receiver<String>,
    sender: Sender<String>,
}

#[wasm_bindgen]
impl Chat {
    // #[wasm_bindgen]
    pub fn send_chat(message: String) -> Result<(), String> {
        todo!()
    }
}
