mod utils;
mod chat;

use futures::channel::mpsc::{self, Receiver, UnboundedReceiver};
use ::futures::{executor, StreamExt, *};
use futures_util::future::{select, Either};
use js_sys::Array;
use libp2p::core::transport::OrTransport;
use libp2p::identify::{Identify, IdentifyEvent, IdentifyConfig};
use libp2p::ping::Ping;
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::wasm_ext::ExtTransport;
use libp2p::Transport;
use utils::set_panic_hook;

use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
// use wasm_rs_async_executor::single_threaded::{run, spawn};

// Exposed Transport Functions
pub use libp2p::wasm_ext::ffi::websocket_transport;
pub use libp2p::wasm_ext::ffi::{Connection, ConnectionEvent, ListenEvent};
use web_sys::{Event, HtmlFormElement};

use ::core::time::Duration;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use gloo::{
    console::{console, log},
    file::BlobContents,
    timers::{
        callback::{Interval, Timeout},
        future::{IntervalStream, TimeoutFuture},
    },
};
use std::borrow::BorrowMut;

use libp2p::{
    core::{self, transport, upgrade::Version::V1Lazy},
    identity,
    identify,
    floodsub::{self, Topic, Floodsub, FloodsubEvent},
    noise::{self, NoiseConfig, X25519Spec},
    ping::{self, PingEvent},
    swarm::{AddressScore, SwarmEvent},
    yamux::YamuxConfig,
    Multiaddr, NetworkBehaviour, PeerId, Swarm,
};

// use libp2p_webrtc::WebRtcTransport;

// #[cfg(not(target_arch = "wasm32"))]
// compile_error("not building wasm!");
// static PEER_CHANNEL: (mpsc::Sender<Multiaddr>, Receiver<Multiaddr>) = mpsc::channel(1);


#[wasm_bindgen(start)]
#[allow(unused_variables)]
pub fn start() {
    set_panic_hook();

    utils::log_meta();

    // add peer channel to user input
    let peer_consumer = attach_add_peer();

    let message_consumer = attach_add_message_box();

    // kickoff async
    start_chat(peer_consumer, message_consumer);
}

fn count_to_60() {
    spawn_local(async {
        let mut count = 0;

        let time = Duration::new(1, 0);
        let mut timer = IntervalStream::new(time.as_millis() as u32);

        // let a = TimeoutFuture::new();

        while count < 60 {
            timer.next().await;
            log!("hello !!!! 1");
            count += 1;
        }
    });
}

fn attach_add_peer() -> UnboundedReceiver<Multiaddr> {
    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");
    let body = document.body().expect("document should have a body");

    let input = document
        .get_element_by_id("input-multiaddr")
        .expect("peer input should be defined")
        .dyn_into::<HtmlFormElement>()
        .expect("elemnt is a form");

    // channel split
    let (mut peer_producer, peer_consumer) = mpsc::unbounded();

    // move producer into closure of input
    let closure = Closure::wrap(Box::new(move |e: Event| {
        //absoulte unit of a call chain
        let input = e
            .current_target()
            .unwrap()
            .dyn_into::<web_sys::HtmlFormElement>()
            .unwrap()
            .get_with_index(0)
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();
        log!(format_args!("user input, add peer: {:?}", input.value()).to_string());
        
        if let Ok(multiaddr) = input.value().parse::<Multiaddr>() {
            peer_producer.unbounded_send(multiaddr).unwrap_throw(); // does this need to be polled?
        } else {
            log!("input peer is invalid or unsupported")
        }
        e.prevent_default();
        
    }) as Box<dyn FnMut(_)>);


    input.set_onsubmit(Some(&closure.as_ref().unchecked_ref()));
    closure.forget();

    peer_consumer
}

fn attach_add_message_box() -> UnboundedReceiver<String> {
    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");
    let body = document.body().expect("document should have a body");

    let input = document
        .get_element_by_id("message")
        .expect("message input should be defined")
        .dyn_into::<HtmlFormElement>()
        .expect("elemnt is a form");

    // channel split
    let (mut message_producer, message_consumer) = mpsc::unbounded();

    // move producer into closure of input
    let closure = Closure::wrap(Box::new(move |e: Event| {
        //absoulte unit of a call chain
        let input = e
            .current_target()
            .unwrap()
            .dyn_into::<web_sys::HtmlFormElement>()
            .unwrap()
            .get_with_index(0)
            .unwrap()
            .dyn_into::<web_sys::HtmlInputElement>()
            .unwrap();
        log!(format_args!("user input, send message: {:?}", input.value()).to_string());
        
        message_producer.unbounded_send(input.value()).unwrap_throw();

        input.set_value(""); 
        e.prevent_default();
        
    }) as Box<dyn FnMut(_)>);


    input.set_onsubmit(Some(&closure.as_ref().unchecked_ref()));
    closure.forget();

    message_consumer
}

fn add_peer(multiaddr: JsValue) -> Result<(), String> {
    if let Some(str) = multiaddr.as_string() {
        if let Ok(multiaddr) = str.parse::<Multiaddr>() {
            Ok(())
        } else {
            Err("Not a valid multiaddr".to_string())
        }
    } else {
        Err("Input is not a string".to_string())
    }
}

fn start_chat(mut peer_consumer: UnboundedReceiver<Multiaddr>, mut message_consumer: UnboundedReceiver<String>) {
    spawn_local(async move {
        // Create a random PeerId
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        log!(format_args!("Local peer id: {:?}", local_peer_id).to_string());

        let topic = Topic::new("chat");

        //transport
        // let webrtc = WebRtcTransport::new(local_peer_id, vec!["stun:stun.l.google.com:19302"]);
        let ws = ExtTransport::new(websocket_transport());

        // TODO: not unwrap
        let noise = noise::Keypair::<noise::X25519Spec>::new()
            .into_authentic(&local_key)
            .unwrap();

        // Behavior
        #[derive(NetworkBehaviour)]
        #[behaviour(out_event = "OutEvent")]
        struct DevBrowserBehavior {
            floodsub: Floodsub,
            ping: Ping, // ping is used to force keepalive during development
            identify: Identify,
        }
        #[derive(Debug)]
        enum OutEvent {
            Floodsub(FloodsubEvent),
            Ping(PingEvent),
            Identify(IdentifyEvent),
        }
        impl From<PingEvent> for OutEvent {
            fn from(v: PingEvent) -> Self {
                Self::Ping(v)
            }
        }
        impl From<FloodsubEvent> for OutEvent {
            fn from(v: FloodsubEvent) -> Self {
                Self::Floodsub(v)
            }
        }
        impl From<IdentifyEvent> for OutEvent {
            fn from(v: IdentifyEvent) -> Self {
                Self::Identify(v)
            }
        }

        // let behaviour = ping::Behaviour::new(ping::Config::new().with_keep_alive(true));
        let behaviour: DevBrowserBehavior = {
            let floodsub = Floodsub::new(local_peer_id);
            let ping = Ping::new(ping::Config::new().with_keep_alive(true));
            let identify = Identify::new(IdentifyConfig::new("1.0.0".to_string(), local_key.public()));
            // subscribes to our topic
            let mut behaviour = DevBrowserBehavior {
                floodsub,
                ping,
                identify
            };
            
            behaviour.floodsub.subscribe(topic.clone());
            behaviour
        };

        // Build transport
        let base = Transport::boxed(ws);
        // let base = OrTransport::new(webrtc, ws);
        let transport = base
            .upgrade(V1Lazy)
            .authenticate(NoiseConfig::xx(noise).into_authenticated())
            .multiplex(YamuxConfig::default())
            .timeout(Duration::from_secs(10))
            .boxed();

        let mut swarm = Swarm::new(transport, behaviour, local_peer_id);

        loop {
            select! {
                event = swarm.select_next_some() => match event {
                    // SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {:?}", address),
                    SwarmEvent::Behaviour(event) => match event {

                        // messages!
                        OutEvent::Floodsub(
                            FloodsubEvent::Message(message)
                        ) => log!(format_args!(
                            "Received: '{:?}' from {:?}",
                            String::from_utf8_lossy(&message.data),
                            message.source
                        ).to_string()),

                        // etc events
                        OutEvent::Floodsub(e) => { log!(format_args!("Floodsub event: {:?}", e).to_string())},
                        e => ()//{ log!(format_args!("Ping event: {:?}", e).to_string())} // print ping events
                    },
                    SwarmEvent::IncomingConnection {
                        local_addr,
                        send_back_addr,
                    } => log!(format_args!(
                        "incoming from:{:?}, sendback: {:?}",
                        local_addr, send_back_addr
                    )
                    .to_string()),
                    SwarmEvent::Dialing(peer_id) => log!(format_args!("dialing {:?}", peer_id).to_string()),

                    //add all new peers to floodsub 
                    SwarmEvent::ConnectionEstablished{peer_id, ..} => {
                        swarm
                        .behaviour_mut()
                        .floodsub
                        .add_node_to_partial_view(peer_id);
                        log!(format_args!("Connected to {:?}", peer_id).to_string());
                    },
                    e => { log!(format_args!("{:?}", e).to_string())}
                },
                peer_multiaddr = peer_consumer.select_next_some() => {
                    match swarm.dial(peer_multiaddr) {
                        Err(e) => log!(format_args!("dial error {:?}", e).to_string()),
                        _ => ()
                    }
                },
                message = message_consumer.select_next_some() => {
                    swarm.behaviour_mut().floodsub.publish(topic.clone(), message);
                },
            }
            
        }
    })
}
