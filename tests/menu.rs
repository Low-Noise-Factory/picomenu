use picomenu::*;
use std::{collections::VecDeque, string::String};

struct MockIo {
    received: VecDeque<String>,
    to_send: VecDeque<String>,
}

impl MockIo {
    fn new() -> Self {
        Self {
            received: Default::default(),
            to_send: Default::default(),
        }
    }

    fn read(&mut self) -> String {
        self.received.pop_front().unwrap()
    }

    fn queue_to_send(&mut self, msg: &str) {
        self.to_send.push_back(msg.to_string());
    }
}

impl IoDevice for MockIo {
    async fn write_packet(&mut self, data: &[u8]) -> Result<(), IoDeviceError> {
        let new_string = String::from_utf8(data.to_vec()).unwrap();
        self.received.push_back(new_string);
        Ok(())
    }

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, IoDeviceError> {
        if let Some(to_send) = self.to_send.pop_front() {
            let bytes_to_send = to_send.as_bytes();
            let len_to_send = bytes_to_send.len();

            if len_to_send < data.len() {
                data[..len_to_send].clone_from_slice(bytes_to_send);
                Ok(len_to_send)
            } else {
                Err(IoDeviceError::BufferOverflow)
            }
        } else {
            Err(IoDeviceError::Disconnected)
        }
    }
}

const TEST_RESPONSE: &str = "Testing 123!\n";
const VERSION_RESPONSE: &str = "Version: 0\n";

struct TestCommand {}
impl<IO: IoDevice> Command<IO, State> for TestCommand {
    fn name() -> &'static str {
        "test"
    }

    fn help_string() -> &'static str {
        "Tests stuff"
    }

    async fn execute(
        _args: Option<&str>,
        output: &mut Output<'_, IO>,
        _state: &mut State,
    ) -> Result<(), MenuError> {
        output.write(TEST_RESPONSE).await?;
        Ok(())
    }
}

struct VersionCommand {}
impl<IO: IoDevice> Command<IO, State> for VersionCommand {
    fn name() -> &'static str {
        "version"
    }

    fn help_string() -> &'static str {
        "Shows version"
    }

    async fn execute(
        _args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut State,
    ) -> Result<(), MenuError> {
        outwriteln!(output, "Version: {}", state.version)
    }
}

struct OverflowCommand {}
impl<IO: IoDevice> Command<IO, State> for OverflowCommand {
    fn name() -> &'static str {
        "overflow"
    }

    fn help_string() -> &'static str {
        "Crashes"
    }

    async fn execute(
        _args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut State,
    ) -> Result<(), MenuError> {
        let res = outwriteln!(output, "Very long text that will overflow");
        state.overflowed = res == Err(MenuError::OutputBufferOverflow);
        Ok(())
    }
}

struct HelloCommand {}
impl<IO: IoDevice> Command<IO, State> for HelloCommand {
    fn name() -> &'static str {
        "hello"
    }

    fn help_string() -> &'static str {
        "Says hello"
    }

    async fn execute(
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        _state: &mut State,
    ) -> Result<(), MenuError> {
        if let Some(name) = args {
            outwriteln!(output, "Hello {}!", name)
        } else {
            outwriteln!(output, "Please enter your name")
        }
    }
}

#[derive(Default)]
struct State {
    version: u32,
    overflowed: bool,
}

fn build_menu<'d>(
    device: &'d mut MockIo,
    state: &'d mut State,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<MockIo, State> + use<'d> {
    make_menu(device, state, input_buffer, output_buffer)
        .with_command::<TestCommand>()
        .with_command::<VersionCommand>()
        .with_command::<OverflowCommand>()
        .with_command::<HelloCommand>()
}

#[tokio::test]
async fn prints_help() {
    let mut device = MockIo::new();
    device.queue_to_send("help\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];

    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), "AVAILABLE COMMANDS:\n\n");
    assert_eq!(device.read(), "> hello: Says hello\n");
    assert_eq!(device.read(), "> overflow: Crashes\n");
    assert_eq!(device.read(), "> version: Shows version\n");
    assert_eq!(device.read(), "> test: Tests stuff\n");
}

#[tokio::test]
async fn supports_simple_command() {
    let mut device = MockIo::new();
    device.queue_to_send("test\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), TEST_RESPONSE);
}

#[tokio::test]
async fn supports_formatting() {
    let mut device = MockIo::new();
    device.queue_to_send("version\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), VERSION_RESPONSE);
}

#[tokio::test]
async fn handles_multiple_requests() {
    let mut device = MockIo::new();
    device.queue_to_send("test\n");
    device.queue_to_send("version\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();

    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), TEST_RESPONSE);
    assert_eq!(device.read(), VERSION_RESPONSE);
}

#[tokio::test]
async fn handles_requests_after_error() {
    let mut device = MockIo::new();
    device.queue_to_send("unkown\n");
    device.queue_to_send("test\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), "Unknown command\n");
    assert_eq!(device.read(), TEST_RESPONSE);
}

#[tokio::test]
async fn supports_inputs_in_pieces() {
    let mut device = MockIo::new();
    device.queue_to_send("tes");
    device.queue_to_send("t\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), TEST_RESPONSE);
}

#[tokio::test]
async fn supports_two_inputs_in_pieces() {
    let mut device = MockIo::new();
    device.queue_to_send("test\nver");
    device.queue_to_send("sion");
    device.queue_to_send("\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), TEST_RESPONSE);
    assert_eq!(device.read(), VERSION_RESPONSE);
}

#[tokio::test]
async fn supports_two_inputs_at_once() {
    let mut device = MockIo::new();
    device.queue_to_send("version\ntest\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), VERSION_RESPONSE);
    assert_eq!(device.read(), TEST_RESPONSE);
}

#[tokio::test]
async fn handles_unknown_command() {
    let mut device = MockIo::new();
    device.queue_to_send("unknown\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), "Unknown command\n");
}

#[tokio::test]
async fn handles_input_buffer_overflow() {
    let mut device = MockIo::new();
    device.queue_to_send("very long string ");
    device.queue_to_send("that will overflow\n");
    device.queue_to_send("test\n");

    let mut input_buffer = [0; 20];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), "Input buffer overflowed & dumped\n");
    assert_eq!(device.read(), TEST_RESPONSE);
}

#[tokio::test]
async fn handles_output_buffer_overflow() {
    let mut device = MockIo::new();
    device.queue_to_send("overflow\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 5];
    let mut state = State::default();
    assert!(!state.overflowed);

    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );

    menu.run().await.unwrap();
    assert!(state.overflowed);
}

#[tokio::test]
async fn handles_command_arguments() {
    let mut device = MockIo::new();
    device.queue_to_send("hello Testing Person\n");

    let mut input_buffer = [0; 128];
    let mut output_buffer = [0; 128];
    let mut state = State::default();
    let menu = build_menu(
        &mut device,
        &mut state,
        &mut input_buffer,
        &mut output_buffer,
    );
    menu.run().await.unwrap();

    assert_eq!(device.read(), "Hello Testing Person!\n");
}
