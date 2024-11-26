#![no_std]

use core::future::Future;
use core::marker::PhantomData;
use core::str::{self, Utf8Error};
use ufmt::uWrite;

#[derive(Debug)]
pub enum IoDeviceError {
    Disconnected,
    InputBufferOverflow,
}

#[derive(Debug)]
pub struct OutputBufferOverflow {}

enum MenuError {
    UnkownCommand,
    Io(IoDeviceError),
    Utf8,
    OutputOverflow,
}

impl From<IoDeviceError> for MenuError {
    fn from(value: IoDeviceError) -> Self {
        MenuError::Io(value)
    }
}

impl From<Utf8Error> for MenuError {
    fn from(_: Utf8Error) -> Self {
        MenuError::Utf8
    }
}

impl From<OutputBufferOverflow> for MenuError {
    fn from(_: OutputBufferOverflow) -> Self {
        MenuError::OutputOverflow
    }
}

pub trait IoDevice {
    fn write_packet(&mut self, data: &[u8]) -> impl Future<Output = ()>;
    fn read_packet(
        &mut self,
        data: &mut [u8],
    ) -> impl Future<Output = Result<usize, IoDeviceError>>;
}

pub struct Output<'d, IO: IoDevice> {
    io_device: &'d mut IO,
    buffer: &'d mut [u8],
    buffer_idx: &'d mut usize,
}

impl<IO: IoDevice> Output<'_, IO> {
    pub async fn write(&mut self, s: &str) {
        self.io_device.write_packet(s.as_bytes()).await;
    }

    pub async fn flush_buffer(&mut self) {
        self.io_device
            .write_packet(&self.buffer[..*self.buffer_idx])
            .await;

        *self.buffer_idx = 0;
    }
}

impl<IO: IoDevice> uWrite for Output<'_, IO> {
    type Error = OutputBufferOverflow;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        let bytes = s.as_bytes();

        let start_idx = *self.buffer_idx;
        if start_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        let end_idx = start_idx + bytes.len();
        if end_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        self.buffer[start_idx..end_idx].clone_from_slice(bytes);
        *self.buffer_idx = end_idx;
        Ok(())
    }
}

#[macro_export]
macro_rules! outwriteln {
    ($out:expr, $($tt:tt)*) => {{
        match ufmt::uwriteln!($out, $($tt)*) {
            Ok(_) => { $out.flush_buffer().await; Ok(()) },
            e => e,
        }
    }}
}

trait Router<IO: IoDevice, S> {
    async fn execute_or_forward(
        &self,
        cmd: &str,
        args: Option<&str>,
        output: &mut Output<IO>,
        state: &mut S,
    ) -> Result<(), MenuError>;

    async fn print_help(&self, output: &mut Output<IO>) -> Result<(), MenuError>;
}

pub trait Command<IO: IoDevice, S> {
    fn execute(
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> impl Future<Output = ()>;
    fn help_string() -> &'static str;
}

struct CommandHolder<IO: IoDevice, S, CMD: Command<IO, S>> {
    name: &'static str,
    _cmd_marker: PhantomData<CMD>,
    _io_marker: PhantomData<IO>,
    _state_marker: PhantomData<S>,
}

impl<IO: IoDevice, S, CMD: Command<IO, S>> CommandHolder<IO, S, CMD> {
    async fn try_execute(
        &self,
        cmd: &str,
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> Result<(), ()> {
        if cmd == self.name {
            CMD::execute(args, output, state).await;
            Ok(())
        } else {
            Err(())
        }
    }

    async fn print_help(&self, output: &mut Output<'_, IO>) -> Result<(), OutputBufferOverflow> {
        outwriteln!(output, "{}: {}", self.name, CMD::help_string())
    }
}

impl<IO: IoDevice, S, CMD: Command<IO, S>> CommandHolder<IO, S, CMD> {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            _cmd_marker: PhantomData,
            _io_marker: PhantomData,
            _state_marker: PhantomData,
        }
    }
}

pub struct NullRouter {}

impl<IO: IoDevice, S> Router<IO, S> for NullRouter {
    async fn execute_or_forward(
        &self,
        _cmd: &str,
        _args: Option<&str>,
        _output: &mut Output<'_, IO>,
        _state: &mut S,
    ) -> Result<(), MenuError> {
        Err(MenuError::UnkownCommand)
    }

    async fn print_help(&self, _output: &mut Output<'_, IO>) -> Result<(), MenuError> {
        Ok(())
    }
}

struct RouterImpl<IO: IoDevice, S, NextRouter: Router<IO, S>, CMD: Command<IO, S>> {
    cmd: CommandHolder<IO, S, CMD>,
    next_router: NextRouter,
}

impl<IO: IoDevice, S, NextRouter: Router<IO, S>, CMD: Command<IO, S>> Router<IO, S>
    for RouterImpl<IO, S, NextRouter, CMD>
{
    async fn execute_or_forward(
        &self,
        cmd: &str,
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> Result<(), MenuError> {
        if self.cmd.try_execute(cmd, args, output, state).await.is_ok() {
            Ok(())
        } else {
            self.next_router
                .execute_or_forward(cmd, args, output, state)
                .await
        }
    }

    async fn print_help(&self, output: &mut Output<'_, IO>) -> Result<(), MenuError> {
        self.cmd.print_help(output).await?;
        self.next_router.print_help(output).await
    }
}

pub trait Menu<IO: IoDevice, S> {
    fn add_command<CMD: Command<IO, S>>(self, name: &'static str) -> impl Menu<IO, S>;
    fn can_run(&mut self) -> impl Future<Output = bool>;
    fn borrow_state(&self) -> &S;
    fn borrow_state_mut(&mut self) -> &mut S;
}

impl<IO: IoDevice, S, HeadRouter: Router<IO, S>> Menu<IO, S> for MenuImpl<'_, IO, S, HeadRouter> {
    fn add_command<CMD: Command<IO, S>>(self, name: &'static str) -> impl Menu<IO, S> {
        assert_ne!(name, "help");

        let new_router = RouterImpl {
            cmd: CommandHolder::<IO, S, CMD>::new(name),
            next_router: self.head_router,
        };

        MenuImpl {
            head_router: new_router,
            input_buffer: self.input_buffer,
            input_buffer_idx: self.input_buffer_idx,
            output_buffer: self.output_buffer,
            output_buffer_idx: self.output_buffer_idx,
            io_device: self.io_device,
            state: self.state,
        }
    }

    async fn can_run(&mut self) -> bool {
        if let Err(e) = self.read_input().await {
            match e {
                MenuError::Io(IoDeviceError::Disconnected) => {
                    return false;
                }
                MenuError::UnkownCommand => {
                    self.println("Unknown command").await;
                }
                MenuError::Io(IoDeviceError::InputBufferOverflow) => {
                    self.println("Input buffer overflow").await;
                }
                MenuError::Utf8 => {
                    self.println("Input UTF8 error").await;
                }
                MenuError::OutputOverflow => {
                    self.println("Output buffer overflow").await;
                }
            }
        }

        true
    }

    fn borrow_state(&self) -> &S {
        &self.state
    }

    fn borrow_state_mut(&mut self) -> &mut S {
        &mut self.state
    }
}

struct MenuImpl<'d, IO: IoDevice, S, HeadRouter: Router<IO, S>> {
    head_router: HeadRouter,
    input_buffer: &'d mut [u8],
    input_buffer_idx: usize,
    output_buffer: &'d mut [u8],
    output_buffer_idx: usize,
    io_device: &'d mut IO,
    state: S,
}

fn parse_line(cmd_string: &[u8]) -> Result<(&str, Option<&str>), Utf8Error> {
    let mut space_idx = 0;

    for (i, char) in cmd_string.iter().enumerate() {
        if *char == b' ' {
            space_idx = i;
            break;
        }
    }

    let after_space_idx = space_idx + 1;

    if space_idx > 0 && after_space_idx < cmd_string.len() {
        let cmd = str::from_utf8(&cmd_string[..space_idx])?;
        let args = str::from_utf8(&cmd_string[after_space_idx..])?;
        Ok((cmd, Some(args)))
    } else {
        let cmd = str::from_utf8(cmd_string)?;
        Ok((cmd, None))
    }
}

impl<'d, IO: IoDevice, S, HeadRouter: Router<IO, S>> MenuImpl<'d, IO, S, HeadRouter> {
    async fn read_input(&mut self) -> Result<(), MenuError> {
        if self.input_buffer_idx >= self.input_buffer.len() {
            return Err(IoDeviceError::InputBufferOverflow.into());
        }

        let n = {
            let buf = &mut self.input_buffer[self.input_buffer_idx..];
            self.io_device.read_packet(buf).await?
        };

        self.input_buffer_idx += n;
        self.process_lines_in_buffer().await
    }

    async fn process_lines_in_buffer(&mut self) -> Result<(), MenuError> {
        let mut output = Output {
            io_device: self.io_device,
            buffer: self.output_buffer,
            buffer_idx: &mut self.output_buffer_idx,
        };

        let last_line_start_idx = {
            let full_input = &self.input_buffer[..self.input_buffer_idx];
            let iter = full_input.iter().enumerate().filter(|(_, c)| **c == b'\n');

            let mut line_start_idx = 0;
            for (line_end_idx, _) in iter {
                assert!(line_start_idx < full_input.len());

                let line = &full_input[line_start_idx..line_end_idx];
                let (cmd, args) = parse_line(line)?;

                if cmd == "help" {
                    self.head_router.print_help(&mut output).await?;
                } else {
                    self.head_router
                        .execute_or_forward(cmd, args, &mut output, &mut self.state)
                        .await?;
                }

                line_start_idx = line_end_idx + 1;
            }
            line_start_idx
        };

        // Now we need to copy the remaining buffer data that has not been processed yet to the front

        if last_line_start_idx == 0 {
            // We can skip this if the buffer already contains the remaining data
            return Ok(());
        }

        let (buffer_head, buffer_tail) = self.input_buffer.split_at_mut(last_line_start_idx);
        let last_line_len = self.input_buffer_idx - last_line_start_idx;
        buffer_head[..last_line_len].copy_from_slice(&buffer_tail[..last_line_len]);
        self.input_buffer_idx = last_line_len;
        Ok(())
    }

    async fn println(&mut self, msg: &'static str) {
        let mut output = Output {
            io_device: self.io_device,
            buffer: self.output_buffer,
            buffer_idx: &mut self.output_buffer_idx,
        };

        outwriteln!(output, "{}", msg).unwrap();
    }
}

pub fn new_menu<'d, IO: IoDevice, S>(
    io_device: &'d mut IO,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
    state: S,
) -> impl Menu<IO, S> + use<'d, IO, S> {
    MenuImpl {
        head_router: NullRouter {},
        input_buffer,
        input_buffer_idx: 0,
        output_buffer,
        output_buffer_idx: 0,
        io_device,
        state,
    }
}

pub async fn run_menu<IO: IoDevice, S>(mut menu: impl Menu<IO, S>) -> impl Menu<IO, S> {
    while menu.can_run().await {}
    menu
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn splits_cmd_string() {
        let test_str = "mycommand random args";
        let (cmd, args) = parse_line(test_str.as_bytes()).unwrap();
        assert_eq!(cmd, "mycommand");
        assert_eq!(args, Some("random args"));
    }

    #[test]
    fn splits_cmd_string_without_args() {
        let test_str = "mycommand";
        let (cmd, args) = parse_line(test_str.as_bytes()).unwrap();
        assert_eq!(cmd, "mycommand");
        assert_eq!(args, None);
    }
}
