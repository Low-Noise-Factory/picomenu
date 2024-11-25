use picomenu::*;
use std::string::String;

struct MockIo {
    received: String,
    to_send: String,
    sent_bytes: usize,
}

impl MockIo {
    fn new(msg: &str) -> Self {
        Self {
            received: Default::default(),
            to_send: msg.to_string(),
            sent_bytes: 0,
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
        let bytes = self.to_send.as_bytes();
        let len = bytes.len();

        if len == self.sent_bytes {
            Err(IoDeviceError::Disconnected)
        } else {
            data[..len].clone_from_slice(bytes);
            self.sent_bytes = len;
            Ok(len)
        }
    }
}

const HELP_RESPONSE: &str = "Help requested!\n";

struct HelpCommand {}
impl<T: IoDevice> Command<T> for HelpCommand {
    async fn execute(&self, output: &mut Output<'_, T>) {
        output.write(HELP_RESPONSE).await;
    }
}

struct VersionCommand {
    version: i32,
}
impl<T: IoDevice> Command<T> for VersionCommand {
    async fn execute(&self, output: &mut Output<'_, T>) {
        outwriteln!(output, "Version: {}", self.version).unwrap();
    }
}

fn build_menu<'d>(
    device: &'d mut MockIo,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<MockIo> + use<'d> {
    new_menu(device, input_buffer, output_buffer)
        .add_command("help", HelpCommand {})
        .add_command("version", VersionCommand { version: 2 })
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

// FIXME: add test for input & output buffer overflows

// FIXME: add test for unkown command
