#![no_std]

use core::future::Future;
use core::marker::PhantomData;
use core::str::{self, Utf8Error};
use ufmt::uWrite;

// Re-export so consumers do not need to depend on `ufmt` directly.
pub use ufmt::uwriteln;

/// These are errors that an `IoDevice` may throw when it is requested to
/// perform an operation.
#[derive(Debug, defmt::Format, PartialEq)]
pub enum IoDeviceError {
    /// This error needs to be thrown when the `IoDevice` has disconnected
    /// and can therefore no longer provide input.
    Disconnected,

    /// This error indicates that the `IoDevice` has experienced an internal
    /// buffer overflow condition.
    BufferOverflow,
}

/// Possible errors that the `Menu` might encounter while running.
#[derive(Debug, defmt::Format, PartialEq)]
pub enum MenuError {
    /// A command was received that was not recognised.
    UnknownCommand,

    /// The `IoDevice` experienced an error while reading or writing.
    Io(IoDeviceError),

    /// Bytes could not be interpreted as valid UTF8.
    Utf8,

    /// `Output` ran out of output buffer space while formatting a string.
    ///
    /// Increasing the size of the menu's output buffer could prevent this.
    OutputBufferOverflow,

    /// `Menu` ran out of input buffer space while reading from its `IoDevice`.
    ///
    /// Increasing the size of the menu's input buffer could prevent this.
    InputBufferOverflow,
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

/// A `struct` that implements the `IoDevice` trait allows a menu to interact with the outside world.
/// This is by providing it with inputs and a way for it to deliver outputs.
pub trait IoDevice {
    /// Allows the menu to write a packet of `UTF8` data to the IO device.
    fn write_packet(&mut self, data: &[u8]) -> impl Future<Output = Result<(), IoDeviceError>>;

    /// Allows the menu to read a packet of `UTF8` data from the IO device.
    fn read_packet(
        &mut self,
        data: &mut [u8],
    ) -> impl Future<Output = Result<usize, IoDeviceError>>;
}

/// An Output handle is provided to `Command` callbacks to enable them to write outputs.
pub struct Output<'d, IO: IoDevice> {
    io_device: &'d mut IO,
    buffer: &'d mut [u8],
    buffer_idx: &'d mut usize,
}

impl<IO: IoDevice> Output<'_, IO> {
    /// Writes directly to the menu's `IoDevice`.
    pub async fn write(&mut self, s: &str) -> Result<(), IoDeviceError> {
        self.io_device.write_packet(s.as_bytes()).await
    }

    /// Flushes the internal buffer to the menu's `IoDevice`.
    /// You should probably not be calling this directly.
    pub async fn flush_buffer(&mut self) -> Result<(), IoDeviceError> {
        self.io_device
            .write_packet(&self.buffer[..*self.buffer_idx])
            .await?;

        *self.buffer_idx = 0;
        Ok(())
    }
}

impl<IO: IoDevice> uWrite for Output<'_, IO> {
    type Error = MenuError;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        let bytes = s.as_bytes();

        let start_idx = *self.buffer_idx;
        if start_idx >= self.buffer.len() {
            return Err(MenuError::OutputBufferOverflow);
        }

        let end_idx = start_idx + bytes.len();
        if end_idx >= self.buffer.len() {
            return Err(MenuError::OutputBufferOverflow);
        }

        self.buffer[start_idx..end_idx].clone_from_slice(bytes);
        *self.buffer_idx = end_idx;
        Ok(())
    }
}

/// Macro allows you to write formatted text using an `Output` handle.
#[macro_export]
macro_rules! outwriteln {
    ($out:expr, $($tt:tt)*) => {{
        match uwriteln!($out, $($tt)*) {
            Ok(_) => $out.flush_buffer().await.map_err(|e| MenuError::Io(e)),
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

/// Commands for a menu are specified by providing structs that implement the Command trait.
/// This allows the menu to understand how to implement the command.
pub trait Command<IO: IoDevice, S> {
    /// Returns the name of this command i.e. what needs to be entered to run it.
    fn name() -> &'static str;

    /// Executes the logic of the command. It is provided with an output handle to print outputs
    /// and a state handle to access menu state (as passed in when the menu was created).
    fn execute(
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> impl Future<Output = Result<(), MenuError>>;

    /// Returns the help string that will be printed for this command.
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
    ) -> Result<bool, MenuError> {
        if cmd == self.name {
            CMD::execute(args, output, state).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn print_help(&self, output: &mut Output<'_, IO>) -> Result<(), MenuError> {
        outwriteln!(output, "> {}: {}", self.name, CMD::help_string())
    }
}

impl<IO: IoDevice, S, CMD: Command<IO, S>> CommandHolder<IO, S, CMD> {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            _cmd_marker: PhantomData,
            _io_marker: PhantomData,
            _state_marker: PhantomData,
        }
    }
}

struct FinalRouter {}

impl<IO: IoDevice, S> Router<IO, S> for FinalRouter {
    async fn execute_or_forward(
        &self,
        _cmd: &str,
        _args: Option<&str>,
        _output: &mut Output<'_, IO>,
        _state: &mut S,
    ) -> Result<(), MenuError> {
        Err(MenuError::UnknownCommand)
    }

    async fn print_help(&self, _output: &mut Output<'_, IO>) -> Result<(), MenuError> {
        Ok(())
    }
}

struct NormalRouter<IO: IoDevice, S, NextRouter: Router<IO, S>, CMD: Command<IO, S>> {
    cmd: CommandHolder<IO, S, CMD>,
    next_router: NextRouter,
}

impl<IO: IoDevice, S, NextRouter: Router<IO, S>, CMD: Command<IO, S>> Router<IO, S>
    for NormalRouter<IO, S, NextRouter, CMD>
{
    async fn execute_or_forward(
        &self,
        cmd: &str,
        args: Option<&str>,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> Result<(), MenuError> {
        if self.cmd.try_execute(cmd, args, output, state).await? {
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

/// You probably don't want to implement this trait yourself! This trait is used to make
/// the internal structure of the Menu opaque to the user which is useful for implementing
/// the builder pattern in the way it has been done here.
///
/// What this means is that when you build a Menu, all you know is that you will end up with
/// "something" that has the interface specified by this trait.
pub trait Menu<IO: IoDevice, S> {
    /// Registers a new command with the Menu.
    fn with_command<CMD: Command<IO, S>>(self) -> impl Menu<IO, S>;

    /// Runs the Menu until it encounters an unrecoverable error or its `IODevice` disconnects.
    fn run(self) -> impl Future<Output = Result<(), MenuError>>;
}

impl<IO: IoDevice, S, HeadRouter: Router<IO, S>> Menu<IO, S> for MenuImpl<'_, IO, S, HeadRouter> {
    fn with_command<CMD: Command<IO, S>>(self) -> impl Menu<IO, S> {
        let name = CMD::name();

        // TODO: return errors for invalid commands instead of pannicing
        assert_ne!(name, "help");
        assert!(name.find([' ']).is_none());

        let new_router = NormalRouter {
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

    async fn run(mut self) -> Result<(), MenuError> {
        loop {
            match self.read_input().await {
                Ok(_) => {}
                Err(MenuError::Io(IoDeviceError::Disconnected)) => return Ok(()),
                other => return other,
            }
        }
    }
}

struct MenuImpl<'d, IO: IoDevice, S, HeadRouter: Router<IO, S>> {
    head_router: HeadRouter,
    input_buffer: &'d mut [u8],
    input_buffer_idx: usize,
    output_buffer: &'d mut [u8],
    output_buffer_idx: usize,
    io_device: &'d mut IO,
    state: &'d mut S,
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

async fn try_print_error<IO: IoDevice>(
    output: &mut Output<'_, IO>,
    e: MenuError,
) -> Result<(), MenuError> {
    match e {
        MenuError::Io(IoDeviceError::Disconnected) => Err(e),
        MenuError::UnknownCommand => {
            outwriteln!(output, "Unknown command")
        }
        MenuError::Io(IoDeviceError::BufferOverflow) => {
            outwriteln!(output, "IO buffer overflow")
        }
        MenuError::Utf8 => {
            outwriteln!(output, "Input UTF8 error")
        }
        MenuError::InputBufferOverflow => {
            outwriteln!(output, "Input buffer overflowed & dumped")
        }

        // We need to abort when then output buffer is full since that
        // condition prevents us from outputting an error message.
        MenuError::OutputBufferOverflow => Err(e),
    }
}

impl<IO: IoDevice, S, HeadRouter: Router<IO, S>> MenuImpl<'_, IO, S, HeadRouter> {
    async fn read_input(&mut self) -> Result<(), MenuError> {
        let read_result = {
            if self.input_buffer_idx < self.input_buffer.len() {
                let buf = &mut self.input_buffer[self.input_buffer_idx..];
                self.io_device.read_packet(buf).await.map_err(|e| match e {
                    IoDeviceError::BufferOverflow => MenuError::InputBufferOverflow,
                    other => MenuError::Io(other),
                })
            } else {
                Err(MenuError::InputBufferOverflow)
            }
        };

        match read_result {
            Ok(n_bytes_read) => {
                self.input_buffer_idx += n_bytes_read;
                self.process_lines_in_buffer().await
            }
            Err(e) => {
                self.input_buffer_idx = 0;
                defmt::debug!("Input buffer dumped due to read error");

                let output = &mut Output {
                    io_device: self.io_device,
                    buffer: self.output_buffer,
                    buffer_idx: &mut self.output_buffer_idx,
                };

                // Try to print an error message before giving up
                try_print_error(output, e).await
            }
        }
    }

    async fn process_lines_in_buffer(&mut self) -> Result<(), MenuError> {
        let output = &mut Output {
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

                defmt::trace!("Picomenu processing line: {:?}", line);

                if cmd == "help" {
                    outwriteln!(output, "AVAILABLE COMMANDS:\n")?;
                    self.head_router.print_help(output).await?;
                } else {
                    let res = self
                        .head_router
                        .execute_or_forward(cmd, args, output, self.state)
                        .await;

                    if let Err(e) = res {
                        // Try to print an error message before giving up
                        try_print_error(output, e).await?
                    }
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
}

/// Returns an empty `Menu` that can be extended/customized using the relevant trait functions.
pub fn make_menu<'d, IO: IoDevice, S>(
    io_device: &'d mut IO,
    state: &'d mut S,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<IO, S> + use<'d, IO, S> {
    MenuImpl {
        head_router: FinalRouter {},
        input_buffer,
        input_buffer_idx: 0,
        output_buffer,
        output_buffer_idx: 0,
        io_device,
        state,
    }
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
