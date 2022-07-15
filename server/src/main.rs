// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! A basic chat application demonstrating libp2p and the mDNS and floodsub protocols.
//!
//! Using two terminal windows, start two instances. If you local network allows mDNS,
//! they will automatically connect. Type a message in either terminal and hit return: the
//! message is sent and printed in the other terminal. Close with Ctrl-c.
//!
//! You can of course open more terminal windows and add more participants.
//! Dialing any of the other peers will propagate the new participant to all
//! chat members and everyone will receive all messages.
//!
//! # If they don't automatically connect
//!
//! If the nodes don't automatically connect, take note of the listening addresses of the first
//! instance and start the second with one of the addresses as the first argument. In the first
//! terminal window, run:
//!
//! ```sh
//! cargo run --example chat
//! ```
//!
//! It will print the PeerId and the listening addresses, e.g. `Listening on
//! "/ip4/0.0.0.0/tcp/24915"`
//!
//! In the second terminal window, start a new instance of the example with:
//!
//! ```sh
//! cargo run --example chat -- /ip4/127.0.0.1/tcp/24915
//! ```
//!
//! The two nodes then connect.

use async_std::{io, task};
use futures::{
    prelude::{stream::StreamExt, *},
    select,
};
use libp2p::{
    floodsub::{self, Floodsub, FloodsubEvent},
    gossipsub::{self, Gossipsub, GossipsubEvent, protocol::GossipsubCodec, GossipsubConfig, TopicScoreParams, PeerScoreParams, PeerScoreThresholds, Sha256Topic, ValidationMode},
    ping::{self, Ping, PingEvent},
    identity::{self, Keypair},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    swarm::SwarmEvent,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, identify::{IdentifyEvent, Identify, IdentifyConfig}, websocket, dns, tcp, Transport, noise::{NoiseConfig, self}, yamux::YamuxConfig, core::upgrade,
};
use std::error::Error;
use core::time::Duration;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);


    let noise = noise::Keypair::<noise::X25519Spec>::new()
            .into_authentic(&local_key)
            .unwrap();
    let base = websocket::WsConfig::new(
        dns::DnsConfig::system(tcp::TcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ))
        .await?,
    ).boxed();
    let transport = base
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(NoiseConfig::xx(noise).into_authenticated())
            .multiplex(YamuxConfig::default())
            .timeout(Duration::from_secs(10))
            .boxed();
    // let transport = libp2p::development_transport(local_key.clone()).await?;


    // Create a Floodsub topic
    let floodsub_topic = floodsub::Topic::new("chat");
    let gossipsub_topic = Sha256Topic::new("gossip-chat");
    

    // We create a custom network behaviour that combines floodsub and mDNS.
    // In the future, we want to improve libp2p to make this easier to do.
    // Use the derive to generate delegating NetworkBehaviour impl and require the
    // NetworkBehaviourEventProcess implementations below.
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "OutEvent")]
    struct MyBehaviour {
        floodsub: Floodsub,
        gossipsub: Gossipsub,
        ping: Ping,
        // identify: Identify,

    }

    #[derive(Debug)]
    enum OutEvent {
        Floodsub(FloodsubEvent),
        Gossipsub(GossipsubEvent),
        Ping(PingEvent),
        // Identify(IdentifyEvent),

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
    impl From<GossipsubEvent> for OutEvent {
        fn from(v: GossipsubEvent) -> Self {
            Self::Gossipsub(v)
        }
    }
    // impl From<IdentifyEvent> for OutEvent {
    //     fn from(v: IdentifyEvent) -> Self {
    //         Self::Identify(v)
    //     }
    // }

    // Create a Swarm to manage peers and events
    let mut swarm = {
        // let mdns = task::block_on(Mdns::new(MdnsConfig::default()))?;
        let identify =
        Identify::new(IdentifyConfig::new("1.0.0".to_string(), local_key.public()));

        let ping = Ping::new(ping::Config::new().with_keep_alive(true));


        let cfg = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
            .validation_mode(ValidationMode::Permissive) // This sets the kind of message validation. The default is Strict (enforce message signing)
            // same content will be propagated.
            .build()
            .expect("Valid config");  
        let mut gossipsub = Gossipsub::new(gossipsub::MessageAuthenticity::Signed(local_key), cfg).unwrap();
        // gossipsub.set_topic_params(gossipsub_topic, TopicScoreParams::default());
        // gossipsub.with_peer_score(PeerScoreParams::default(), PeerScoreThresholds::default());
        gossipsub.subscribe(&gossipsub_topic).unwrap();
        

        let mut behaviour = MyBehaviour {
            floodsub: Floodsub::new(local_peer_id),
            gossipsub,
            // identify,
            ping,
        };

        behaviour.floodsub.subscribe(floodsub_topic.clone());
        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Reach out to another node if specified
    if let Some(to_dial) = std::env::args().nth(1) {
        let addr: Multiaddr = to_dial.parse()?;
        swarm.dial(addr)?;
        println!("Dialed {:?}", to_dial)
    }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0/ws".parse()?)?;

    // Kick it off
    loop {
        select! {
            line = stdin.select_next_some() => {
                let line = line.expect("Stdin not to close");
                let b = swarm
                .behaviour_mut();
                b.floodsub
                .publish(floodsub_topic.clone(), line.as_bytes());
                b.gossipsub.publish(gossipsub_topic.clone(), line);
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {:?}", address);
                }
                // SwarmEvent::Behaviour(OutEvent::Floodsub(
                //     FloodsubEvent::Message(message)
                // )) => {
                //     println!(
                //         "Received flood: '{:?}' from {:?}",
                //         String::from_utf8_lossy(&message.data),
                //         message.source
                //     );
                // },
                SwarmEvent::Behaviour(OutEvent::Gossipsub(
                    GossipsubEvent::Message { message, .. }
                )) => {
                    println!(
                        "Received gossip: '{:?}' from {:?}",
                        String::from_utf8_lossy(&message.data),
                        message.source
                    );
                },

                SwarmEvent::ConnectionEstablished{peer_id, ..} => {
                    swarm.behaviour_mut().floodsub.add_node_to_partial_view(peer_id);
                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);

                }
                e => ()//{println!("event {e:?}")}
            }
        }
    }
}
