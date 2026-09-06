use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{proto, MAX_CONTROL_FRAME_SIZE, MAX_RELIABLE_INPUT_FRAME_SIZE};

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error(transparent)]
    Frame(#[from] arcrelay_transport::FrameError),
    #[error("protobuf decode failed: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("protobuf encode failed: {0}")]
    Encode(#[from] prost::EncodeError),
}

pub async fn read_control<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<proto::ControlFrame, WireError> {
    let bytes = arcrelay_transport::read_frame(reader, MAX_CONTROL_FRAME_SIZE).await?;
    Ok(proto::ControlFrame::decode(bytes.as_slice())?)
}

pub async fn write_control<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &proto::ControlFrame,
) -> Result<(), WireError> {
    write_message(writer, frame, MAX_CONTROL_FRAME_SIZE).await
}

pub async fn read_input<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<proto::InputEventBatch, WireError> {
    let bytes = arcrelay_transport::read_frame(reader, MAX_RELIABLE_INPUT_FRAME_SIZE).await?;
    Ok(proto::InputEventBatch::decode(bytes.as_slice())?)
}

pub async fn write_input<W: AsyncWrite + Unpin>(
    writer: &mut W,
    batch: &proto::InputEventBatch,
) -> Result<(), WireError> {
    write_message(writer, batch, MAX_RELIABLE_INPUT_FRAME_SIZE).await
}

async fn write_message<W: AsyncWrite + Unpin, M: Message>(
    writer: &mut W,
    message: &M,
    maximum: usize,
) -> Result<(), WireError> {
    let mut bytes = Vec::with_capacity(message.encoded_len());
    message.encode(&mut bytes)?;
    arcrelay_transport::write_frame(writer, &bytes, maximum).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn control_frames_round_trip() {
        let frame = proto::ControlFrame {
            body: Some(proto::control_frame::Body::EmergencyRelease(
                proto::EmergencyRelease {
                    header: None,
                    reason: "test".into(),
                },
            )),
        };
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_control(&mut writer, &frame).await.unwrap();
        assert_eq!(read_control(&mut reader).await.unwrap(), frame);
    }

    #[tokio::test]
    async fn relayed_peer_routes_round_trip() {
        let frame = proto::ControlFrame {
            body: Some(proto::control_frame::Body::PeerRouteSnapshot(
                proto::PeerRouteSnapshot {
                    header: None,
                    routes: vec![proto::PeerRoute {
                        device_id: "arc-test".into(),
                        endpoints: vec!["10.1.1.122:8765".into()],
                        certificate_sha256: vec![7; 32],
                        input_relay_available: true,
                    }],
                    active_controller_device_id: "arc-controller".into(),
                    active_control_epoch: 42,
                },
            )),
        };
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_control(&mut writer, &frame).await.unwrap();
        assert_eq!(read_control(&mut reader).await.unwrap(), frame);
    }

    #[tokio::test]
    async fn native_quartz_scroll_round_trips_with_portable_fallback() {
        let batch = proto::InputEventBatch {
            header: None,
            events: vec![proto::InputEvent {
                event: Some(proto::input_event::Event::Scroll(proto::Scroll {
                    x_milli: -3_250,
                    y_milli: 12_500,
                    precise: true,
                    phase: 2,
                    native_quartz_event: vec![0x51, 0x55, 0x41, 0x52, 0x54, 0x5a],
                    native_quartz_format_version: crate::NATIVE_QUARTZ_FORMAT_VERSION,
                    unit: proto::ScrollUnit::Pixel as i32,
                    gesture_phase: proto::ScrollGesturePhase::Began as i32,
                    momentum_phase: proto::ScrollMomentumPhase::Changed as i32,
                })),
            }],
            held_state_checksum: 0,
            sent_at_unix_micros: 42,
        };
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_input(&mut writer, &batch).await.unwrap();
        assert_eq!(read_input(&mut reader).await.unwrap(), batch);
    }

    #[tokio::test]
    async fn system_gesture_phases_round_trip_without_native_payloads() {
        let batch = proto::InputEventBatch {
            events: [1, 2, 4, 8]
                .map(|phase| proto::InputEvent {
                    event: Some(proto::input_event::Event::SystemGesture(
                        crate::SystemGestureEvent {
                            axis: 2,
                            phase,
                            progress: -0.415924072,
                            velocity_x: -3.2,
                            velocity_y: -3.2,
                            inverted_from_device: false,
                        }
                        .into(),
                    )),
                })
                .to_vec(),
            ..Default::default()
        };
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_input(&mut writer, &batch).await.unwrap();
        assert_eq!(read_input(&mut reader).await.unwrap(), batch);
        // Old capability frames decode with the opt-in fields disabled.
        let old = proto::CapabilitySnapshot::decode(&[][..]).unwrap();
        assert!(!old.can_capture_system_gestures && !old.can_inject_system_gestures);
        assert_eq!(old.system_gesture_format_version, 0);
    }
}
