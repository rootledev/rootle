//! Generated operation streams run through the production router and an
//! independent reference map. Observations are per-request channel deliveries,
//! not source text or constructor-field echoes.

use super::*;
use std::collections::BTreeMap;

#[test]
fn generated_routing_interleavings_match_the_reference_model() {
    for seed in [1u64, 7, 42, 1337, 99991] {
        let mut random = seed;
        let mut routing = Routing::default();
        let mut old_session = routing.session;
        let mut reference = BTreeMap::<u64, bool>::new();
        let mut receivers = BTreeMap::new();
        let mut next_id = 0;
        for step in 0..400 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let id_value = (random >> 8) % (next_id + 3) + 1;
            let id = RequestId::from_wire(id_value).unwrap();
            let mut expected = None;
            match random % 7 {
                0 | 1 => {
                    let streaming = random % 7 == 1;
                    let (actual, receiver) = routing.register(true, streaming).unwrap();
                    next_id += 1;
                    assert_eq!(actual.wire(), next_id);
                    reference.insert(next_id, streaming);
                    receivers.insert(next_id, receiver);
                }
                2 => {
                    if reference.remove(&id_value).is_some() {
                        expected = Some((id_value, false));
                    }
                    routing.response(
                        routing.session,
                        id,
                        serde_json::json!({"source":id_value,"step":step}),
                    );
                }
                3 => {
                    if reference.get(&id_value) == Some(&true) {
                        expected = Some((id_value, true));
                    }
                    routing.partial(
                        routing.session,
                        id,
                        serde_json::json!({"source":id_value,"step":step}),
                    );
                }
                4 => {
                    reference.remove(&id_value);
                    routing.expire(id);
                }
                5 => {
                    old_session = routing.session;
                    routing.disconnect(old_session);
                    reference.clear();
                    routing.begin_rebuild();
                    // Recovery tests separately exercise the handshake. Here
                    // we admit a validated new reader to test late old work.
                    routing.output = OutputState::Open;
                    routing.lifecycle = Lifecycle::Alive;
                }
                _ => {
                    if old_session != routing.session {
                        routing.disconnect(old_session);
                        routing.response(
                            old_session,
                            id,
                            serde_json::json!({"wrong":"generation"}),
                        );
                        routing.partial(old_session, id, serde_json::json!({"wrong":"generation"}));
                    }
                }
            }
            for (&owner, receiver) in &receivers {
                match (expected, receiver.try_recv()) {
                    (Some((target, partial)), Ok(message)) if owner == target => {
                        let value = match message {
                            Delivery::Partial(value) => {
                                assert!(partial);
                                value
                            }
                            Delivery::Response(value) => {
                                assert!(!partial);
                                value
                            }
                        };
                        assert_eq!(value["source"], owner);
                        assert_eq!(value["step"], step);
                    }
                    (_, Err(mpsc::TryRecvError::Empty)) => {
                        assert!(reference.contains_key(&owner));
                        assert_ne!(expected.map(|(target, _)| target), Some(owner));
                    }
                    (_, Err(mpsc::TryRecvError::Disconnected)) => {
                        assert!(!reference.contains_key(&owner));
                        assert_ne!(expected.map(|(target, _)| target), Some(owner));
                    }
                    _ => panic!("misrouted response: seed {seed}, step {step}, owner {owner}"),
                }
            }
        }
    }
}

#[test]
fn new_reader_eof_does_not_admit_another_rebuilder_during_handshake() {
    let mut routing = Routing::default();
    routing.disconnect(routing.session);
    routing.begin_rebuild();
    let session = routing.session;
    routing.output = OutputState::Open;
    let (_, handshake) = routing.register(false, false).unwrap();
    routing.disconnect(session);
    assert!(matches!(
        handshake.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(routing.lifecycle, Lifecycle::Respawning);
    assert!(routing.register(true, false).is_err());
}
