# Picomenu

[![Build and Test](https://github.com/Low-Noise-Factory/picomenu/actions/workflows/build_and_test.yml/badge.svg)](https://github.com/Low-Noise-Factory/picomenu/actions/workflows/build_and_test.yml)

Picomenu is a very simple library to implement command line menus on no-std + async embedded systems such as those powered by [Embassy](https://embassy.dev/). It is only meant for very simple menus and therefore does not support more advanced features at the moment.

Following is an an example of how to use the library:

```
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

struct State {
    version: u32,
}

fn build_menu<'d>(
    device: &'d mut MockIo,
    state: &'d mut State,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<MockIo, State> + use<'d> {
    make_menu(device, state, input_buffer, output_buffer)
        .with_command::<VersionCommand>()
        .with_command::<HelloCommand>()
}
```

Here is should be noted that the help command is also provided automatically!

To get things working in your system, you will also need to implement the `IoDevice` trait for the struct that is responsible for input to and output from the menu. For more details on this and other aspects of how to use the library, please have a look at `tests/menu.rs`. Finally, you will need to add `ufmt` as a dependency to project as it was unfortunately not possible to avoid having it as a peer dependency.

The current feature set is sufficient for our needs. Therefore, we will unfortunately not have time to address any feature requests :) But please feel free to contribute any features you may need yourself ;)
