use picomenu::*;
use std::string::String;

struct MockIo {
    received: String,
    to_send: String,
    packet_sent: bool,
}

impl MockIo {
    fn new(msg: &str) -> Self {
        Self {
            received: Default::default(),
            to_send: msg.to_string(),
            packet_sent: false,
        }
    }

    fn read(&self) -> &str {
        &self.received
    }
}

impl IoDevice for MockIo {
    async fn write_packet(&mut self, data: &[u8]) {
        self.received = String::from_utf8(data.to_vec()).unwrap();
    }

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, IoDeviceError> {
        if self.packet_sent {
            return Err(IoDeviceError::Disconnected);
        } else {
            self.packet_sent = true;
        }

        let bytes_to_send = self.to_send.as_bytes();
        let len_to_send = bytes_to_send.len();

        if len_to_send < data.len() {
            data[..len_to_send].clone_from_slice(bytes_to_send);
            Ok(len_to_send)
        } else {
            Err(IoDeviceError::InputBufferOverflow)
        }
    }
}

const HELP_RESPONSE: &str = "Help requested!\n";

struct HelpCommand {}
impl<T: IoDevice> Command<T> for HelpCommand {
    async fn execute(output: &mut Output<'_, T>) {
        output.write(HELP_RESPONSE).await;
    }
}

struct VersionCommand {}
impl<T: IoDevice> Command<T> for VersionCommand {
    async fn execute(output: &mut Output<'_, T>) {
        outwriteln!(output, "Version: {}", 2).unwrap();
    }
}

fn build_menu<'d>(
    device: &'d mut MockIo,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<MockIo> + use<'d> {
    new_menu(device, input_buffer, output_buffer)
        .add_command::<HelpCommand>("help")
        .add_command::<VersionCommand>("version")
}

#[tokio::test]
async fn shows_help() {
    let mut device = MockIo::new("help\n");
    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let menu = build_menu(&mut device, &mut input_buffer, &mut output_buffer);

    run_menu(menu).await;
    assert_eq!(device.read(), HELP_RESPONSE);
}

#[tokio::test]
async fn shows_version() {
    let mut device = MockIo::new("version\n");
    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let menu = build_menu(&mut device, &mut input_buffer, &mut output_buffer);

    run_menu(menu).await;
    assert_eq!(device.read(), "Version: 2\n");
}

#[tokio::test]
async fn handles_unknown_command() {
    let mut device = MockIo::new("unknown\n");
    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let menu = build_menu(&mut device, &mut input_buffer, &mut output_buffer);

    run_menu(menu).await;
    assert_eq!(device.read(), "Unknown command\n");
}

#[tokio::test]
async fn handles_input_buffer_overflow() {
    let mut device = MockIo::new("very long string that will overflow\n");
    let mut input_buffer = [0; 5];
    let mut output_buffer = [0; 128];
    let menu = build_menu(&mut device, &mut input_buffer, &mut output_buffer);

    run_menu(menu).await;
    assert_eq!(device.read(), "Input buffer overflow\n");
}

// FIXME: add test for output buffer overflow
