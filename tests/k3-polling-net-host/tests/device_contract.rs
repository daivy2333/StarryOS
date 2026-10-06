//! Task 3.1 witness: the production polling `Device` token reservation,
//! backpressure and fault semantics against the fake board.

use smoltcp::{phy::{Device as _, RxToken as _, TxToken as _}, time::Instant as SmolInstant};

use k3_polling_net_host::fake::{self, FakeEnv};
use k3_polling_net_host::k3_gmac::{
    polling_net::{K3PollDevice, SessionFaultDetail, SessionFaultStage},
    DMA_CHAN_STATUS, DMA_STATUS_FBE, GmacError,
};

const TS: SmolInstant = SmolInstant::from_millis_const(0);

fn checksum16(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in data.chunks(2) {
        let w = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]]) as u32
        } else {
            (chunk[0] as u32) << 8
        };
        sum += w;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Full Ethernet + IPv4 + ICMP echo request frame (correct checksums: the
/// board verifies in software).
fn icmp_echo_frame(ident: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&fake::BOARD_MAC);
    f.extend_from_slice(&fake::PEER_MAC);
    f.extend_from_slice(&[0x08, 0x00]);
    let icmp_len = 8 + payload.len();
    f.extend_from_slice(&[0x45, 0x00]);
    f.extend_from_slice(&((20 + icmp_len) as u16).to_be_bytes());
    f.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 64, 1, 0x00, 0x00]);
    f.extend_from_slice(&fake::PEER_IP);
    f.extend_from_slice(&fake::BOARD_IP);
    let ip_csum = checksum16(&f[14..34]);
    f[24..26].copy_from_slice(&ip_csum.to_be_bytes());
    let icmp_start = f.len();
    f.extend_from_slice(&[8, 0, 0, 0]);
    f.extend_from_slice(&ident.to_be_bytes());
    f.extend_from_slice(&seq.to_be_bytes());
    f.extend_from_slice(payload);
    let c = checksum16(&f[icmp_start..]);
    f[icmp_start + 2..icmp_start + 4].copy_from_slice(&c.to_be_bytes());
    f
}

/// Ethernet + ARP who-has BOARD_IP (from the peer).
fn arp_request_frame() -> Vec<u8> {
    let mut f = vec![0u8; 60];
    f[0..6].fill(0xff);
    f[6..12].copy_from_slice(&fake::PEER_MAC);
    f[12] = 0x08;
    f[13] = 0x06;
    f[14] = 0x00;
    f[15] = 0x01;
    f[16] = 0x08;
    f[17] = 0x00;
    f[18] = 6;
    f[19] = 4;
    f[21] = 1;
    f[22..28].copy_from_slice(&fake::PEER_MAC);
    f[28..32].copy_from_slice(&fake::PEER_IP);
    f[38..42].copy_from_slice(&fake::BOARD_IP);
    f
}

fn fill_tx(device: &mut K3PollDevice<fake::LeakBacking, FakeEnv>, fill: u8) {
    for _ in 0..16 {
        let token = device.transmit(TS).expect("free capacity");
        token.consume(60, |buf| buf.fill(fill));
    }
}

#[test]
fn tx_capacity_full_refuses_without_touching_anything() {
    let mut b = FakeEnv::board();
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    assert_eq!(device.tx_free_capacity(), 16);
    fill_tx(&mut device, 0xaa);
    assert_eq!(device.tx_free_capacity(), 0);
    assert!(device.transmit(TS).is_none(), "Full never hands out a token");
    assert!(device.take_fault().is_none(), "Full is backpressure, not a fault");
    // The device completes all 16 on the wire; the next device entry reclaims.
    assert_eq!(b.wire.drain_board_tx().len(), 16);
    assert!(device.receive(TS).is_none());
    assert_eq!(device.tx_free_capacity(), 16);
}

#[test]
fn unconsumed_tx_token_releases_the_reservation() {
    let mut b = FakeEnv::board();
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    let token = device.transmit(TS).expect("free capacity");
    drop(token);
    assert_eq!(device.tx_free_capacity(), 16, "unconsumed token frees its slot");
    assert!(b.wire.drain_board_tx().is_empty(), "nothing was published");
    assert!(device.take_fault().is_none());
}

#[test]
fn rx_pair_delivered_with_capacity_and_icmp_classified() {
    let mut b = FakeEnv::board();
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    let frame = icmp_echo_frame(0x1234, 7, b"ms11-ping");
    b.wire.inject_rx_frame(&frame);
    let (rx, _tx) = device.receive(TS).expect("pair while TX capacity is free");
    let mut got = Vec::new();
    rx.consume(|bytes| got.extend_from_slice(bytes));
    // The wire pads sub-60-byte frames: the delivered copy is the padded
    // frame (FCS already stripped by the driver), prefix-identical.
    assert_eq!(&got[..frame.len()], &frame[..], "the stashed copy is byte-identical");
    assert_eq!(got.len(), 60);
    let counters = device.counters();
    assert_eq!(counters.rx_frames, 1);
    assert_eq!(counters.icmp_echo_replies, 1, "each echo request is one auto-reply");
    assert_eq!(device.tx_free_capacity(), 16, "unconsumed tx token of the pair freed");
    // The stash is empty now: the next receive is idle.
    assert!(device.receive(TS).is_none());
}

#[test]
fn rx_frame_retained_when_tx_is_full_then_delivered() {
    let mut b = FakeEnv::board();
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    fill_tx(&mut device, 0x55);
    let frame = arp_request_frame();
    b.wire.inject_rx_frame(&frame);
    assert!(device.receive(TS).is_none(), "no TX capacity: the pair is withheld");
    assert_eq!(device.counters().rx_frames, 0, "stashed frame is not yet delivered");
    assert_eq!(b.wire.drain_board_tx().len(), 16);
    // Reclaim runs at the next device entry, then the retained frame ships.
    let (rx, _tx) = device.receive(TS).expect("retained frame delivered after reclaim");
    let mut got = Vec::new();
    rx.consume(|bytes| got.extend_from_slice(bytes));
    assert_eq!(got, frame, "retention preserved the exact bytes");
    assert_eq!(device.counters().rx_frames, 1);
}

#[test]
fn short_tx_frame_records_fault_and_never_publishes() {
    let mut b = FakeEnv::board();
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    let token = device.transmit(TS).expect("free capacity");
    token.consume(10, |buf| buf.fill(0));
    let fault = device.take_fault().expect("consume cannot fail silently");
    assert!(matches!(fault.stage, SessionFaultStage::DeviceTxSubmit));
    assert!(matches!(fault.detail, SessionFaultDetail::TxLengthInvalid { len: 10 }));
    assert!(b.wire.drain_board_tx().is_empty(), "no wire frame for a refused submit");
    assert_eq!(device.tx_free_capacity(), 16, "the refused slot was never owned");
}

#[test]
fn fatal_bus_error_surfaces_as_device_fault() {
    let mut b = FakeEnv::board();
    b.env.set_reg(DMA_CHAN_STATUS, DMA_STATUS_FBE);
    let mut device = K3PollDevice::new(b.ready_driver(), b.env);
    assert!(device.receive(TS).is_none());
    let fault = device.take_fault().expect("FBE is recorded");
    assert!(matches!(fault.stage, SessionFaultStage::DeviceTxReclaim));
    assert!(matches!(fault.detail, SessionFaultDetail::Driver(GmacError::Fault { .. })));
    assert!(device.is_faulted(), "the device stays faulted after the report");
    // Device-owned backing is never released by a fault.
    assert!(device.backing_retained());
}
