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
impl<T: IoDevice> Execute<T> for HelpCommand {
    async fn exe(&self, output: &mut Output<'_, T>) {
        output.write(HELP_RESPONSE).await;
    }
}

struct VersionCommand {
    version: i32,
}
impl<T: IoDevice> Execute<T> for VersionCommand {
    async fn exe(&self, output: &mut Output<'_, T>) {
        outwriteln!(output, "Version: {}", self.version);
    }
}

#[tokio::test]
async fn shows_help() {
    let mut device = MockIo::new("help\n");

    let menu = new_menu(&mut device)
        .add_command(Command::new("help", HelpCommand {}))
        .add_command(Command::new("version", VersionCommand { version: 2 }));

    run_menu(menu).await;
    assert_eq!(device.read(), HELP_RESPONSE);
}

#[tokio::test]
async fn shows_version() {
    let mut device = MockIo::new("version\n");

    let menu = new_menu(&mut device)
        .add_command(Command::new("help", HelpCommand {}))
        .add_command(Command::new("version", VersionCommand { version: 2 }));

    run_menu(menu).await;
    assert_eq!(device.read(), "Version: 2\n");
}
