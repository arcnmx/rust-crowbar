//! crowbar makes it easy to write AWS Lambda functions in Rust. It wraps native Rust functions
//! into CPython modules that handle converting Python objects into Rust objects and back again.
//!
//! # Usage
//!
//! Add both crowbar and cpython to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! crowbar = "0.1"
//! cpython = { version = "*", default-features = false, features = ["python27-sys"] }
//! ```
//!
//! Use macros from both crates:
//!
//! ```rust,ignore
//! #[macro_use(lambda)]
//! extern crate crowbar;
//! #[macro_use]
//! extern crate cpython;
//! ```
//!
//! And write your function using the `lambda!` macro:
//!
//! ```rust
//! # #[macro_use(lambda)] extern crate crowbar;
//! # #[macro_use] extern crate cpython;
//! # fn main() {
//! lambda!(|event, context| {
//!     println!("hi cloudwatch logs, this is {}", context.function_name());
//!     // return the event without doing anything with it
//!     Ok(event)
//! });
//! # }
//! ```
//!
//! # Building Lambda functions
//!
//! For your code to be usable in AWS Lambda's Python execution environment, you need to compile to
//! a dynamic library with the necessary functions for CPython to run. The `lambda!` macro does
//! most of this for you, but cargo still needs to know what to do.
//!
//! You can configure cargo to build a dynamic library with the following. Note that the library
//! name *must* be `lambda`.
//!
//! ```toml
//! [lib]
//! name = "lambda"
//! crate-type = ["cdylib"]
//! ```
//!
//! `cargo build` will now build a `liblambda.so`. Put this in a zip file and upload it to an AWS
//! Lambda function. You will need to use the Python 2.7 execution environment with the handler
//! configured as `liblambda.handler`.
//!
//! For best results, it's important to build the shared library on a system using the same
//! libraries as the Lambda execution environment. Since Lambda uses Amazon Linux, the easiest way
//! to do this is to use an [EC2 instance](https://aws.amazon.com/amazon-linux-ami/) or a [Docker
//! container](https://hub.docker.com/_/amazonlinux/).
//!
//! The `builder` directory of the [crowbar git repo](https://github.com/ilianaw/rust-crowbar)
//! contains a `Dockerfile` with Rust set up and a build script to dump a zip file containing a
//! stripped shared library to stdout. Documentation for that is available at
//! [ilianaw/crowbar-builder on Docker Hub](https://hub.docker.com/r/ilianaw/crowbar-builder/).

extern crate cpython;
extern crate cpython_json;
extern crate serde_json;

#[doc(hidden)]
pub use cpython::{PyResult, PyObject};
pub use serde_json::value::Value;

/// Result object that accepts `Ok(Value)` or any `Err(Error)`.
///
/// crowbar uses [the `Box<Error>` method of error handling]
/// (https://doc.rust-lang.org/stable/book/error-handling.html#error-handling-with-boxerror) so
/// that any `Error` can be thrown within your Lambda function.
///
/// If an error is thrown, it is converted to a Python `RuntimeError`, and the `Debug` string for
/// the `Error` returned is used as the value.
pub type LambdaResult = Result<Value, Box<std::error::Error>>;

use cpython::{Python, PyUnicode, PyTuple, PyErr, PythonObject, PythonObjectWithTypeObject,
              ObjectProtocol};
use cpython_json::{from_json, to_json};

/// Provides a view into the `context` object available to Lambda functions.
///
/// Context object methods and attributes are documented at [The Context Object (Python)]
/// (https://docs.aws.amazon.com/lambda/latest/dg/python-context-object.html) from the AWS Lambda
/// docs.
pub struct LambdaContext<'a> {
    py: &'a Python<'a>,
    py_context: &'a PyObject,
    string_storage: [String; 7],
}

impl<'a> LambdaContext<'a> {
    fn new(py: &'a Python, py_context: &'a PyObject) -> PyResult<LambdaContext<'a>> {
        macro_rules! str_attr {
            ($x:expr) => {
                py_context.getattr(*py, $x)?.extract::<String>(*py)?;
            }
        }

        let string_storage: [String; 7] = [str_attr!("function_name"),
                                           str_attr!("function_version"),
                                           str_attr!("invoked_function_arn"),
                                           str_attr!("memory_limit_in_mb"),
                                           str_attr!("aws_request_id"),
                                           str_attr!("log_group_name"),
                                           str_attr!("log_stream_name")];

        Ok(LambdaContext {
            py: py,
            py_context: py_context,
            string_storage: string_storage,
        })
    }

    /// Name of the Lambda function that is executing.
    pub fn function_name(&self) -> &str {
        &self.string_storage[0]
    }

    /// The Lambda function version that is executing. If an alias is used to invoke the function,
    /// then `function_version` will be the version the alias points to.
    pub fn function_version(&self) -> &str {
        &self.string_storage[1]
    }

    /// The ARN used to invoke this function. It can be function ARN or alias ARN. An unqualified
    /// ARN executes the `$LATEST` version and aliases execute the function version it is pointing
    /// to.
    pub fn invoked_function_arn(&self) -> &str {
        &self.string_storage[2]
    }

    /// Memory limit, in MB, you configured for the Lambda function. You set the memory limit at
    /// the time you create a Lambda function and you can change it later.
    pub fn memory_limit_in_mb(&self) -> &str {
        &self.string_storage[3]
    }

    /// AWS request ID associated with the request. This is the ID returned to the client that
    /// called the invoke method.
    ///
    /// **Note**: If AWS Lambda retries the invocation (for example, in a situation where the
    /// Lambda function that is processing Amazon Kinesis records throws an exception), the request
    /// ID remains the same.
    pub fn aws_request_id(&self) -> &str {
        &self.string_storage[4]
    }

    /// The name of the CloudWatch log group where you can find logs written by your Lambda
    /// function.
    pub fn log_group_name(&self) -> &str {
        &self.string_storage[5]
    }

    /// The name of the CloudWatch log stream where you can find logs written by your Lambda
    /// function. The log stream may or may not change for each invocation of the Lambda function.
    ///
    /// The value is null if your Lambda function is unable to create a log stream, which can
    /// happen if the execution role that grants necessary permissions to the Lambda function does
    /// not include permissions for the CloudWatch Logs actions.
    pub fn log_stream_name(&self) -> &str {
        &self.string_storage[6]
    }

    /// Returns the remaining execution time, in milliseconds, until AWS Lambda terminates the
    /// function.
    ///
    /// This returns `ContextError::GetRemainingTimeFailed` if crowbar is unable to call the method
    /// or cast it to a `u64` from the Python object. This should generally never happen, so you
    /// should simply call this as `context.get_remaining_time_in_millis()?` in your function.
    pub fn get_remaining_time_in_millis(&self) -> Result<u64, ContextError> {
        self.py_context
            .call_method(*self.py,
                         "get_remaining_time_in_millis",
                         PyTuple::new(*self.py, &[]),
                         None)
            .and_then(|x| x.extract::<u64>(*self.py))
            .map_err(|_| ContextError::GetRemainingTimeFailed)
    }
}

/// Error enum for things that can go wrong while processing the context object.
#[derive(Debug)]
pub enum ContextError {
    /// Occurs if crowbar is unable to call the method on the context object or cast it to a `u64`
    /// from the Python object.
    GetRemainingTimeFailed,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            ContextError::GetRemainingTimeFailed => {
                write!(f, "failed to call get_remaining_time_in_millis")
            }
        }
    }
}

impl std::error::Error for ContextError {
    fn description(&self) -> &str {
        match *self {
            ContextError::GetRemainingTimeFailed => "failed to call get_remaining_time_in_millis",
        }
    }

    fn cause(&self) -> Option<&std::error::Error> {
        None
    }
}

#[doc(hidden)]
pub fn handler<F>(py: Python, f: F, py_event: PyObject, py_context: PyObject) -> PyResult<PyObject>
    where F: Fn(Value, LambdaContext) -> LambdaResult
{
    let event = to_json(py, &py_event).or_else(|e| Err(e.to_pyerr(py)))?;
    let result = match f(event, LambdaContext::new(&py, &py_context)?) {
        Ok(r) => r,
        Err(e) => {
            return Err(PyErr {
                ptype: cpython::exc::RuntimeError::type_object(py).into_object(),
                pvalue: Some(PyUnicode::new(py, &format!("{:?}", e)).into_object()),
                ptraceback: None,
            })
        }
    };
    from_json(py, result).or_else(|e| Err(e.to_pyerr(py)))
}

#[macro_export]
/// Macro to wrap a Lambda function handler.
///
/// Lambda functions accept two arguments (the event, a serde_json `Value`, and the context, a
/// `LambdaContext`) and returns a vluae (a serde_json `Value`). The function signature should look
/// like:
///
/// ```rust,ignore
/// fn handler(event: Value, context: LambdaContext) -> LambdaResult
/// ```
///
/// To use this macro, you need to `macro_use` both crowbar *and* cpython, because crowbar
/// references multiple cpython macros.
///
/// ```rust,ignore
/// #[macro_use(lambda)]
/// extern crate crowbar;
/// #[macro_use]
/// extern crate cpython;
/// ```
///
/// # Examples
///
/// You can wrap a closure with `lambda!`:
///
/// ```rust
/// # #[macro_use(lambda)] extern crate crowbar;
/// # #[macro_use] extern crate cpython;
/// # fn main() {
/// lambda!(|event, context| {
///     println!("hello!");
///     Ok(event)
/// });
/// # }
/// ```
///
/// You can also define a named function:
///
/// ```rust
/// # #[macro_use(lambda)] extern crate crowbar;
/// # #[macro_use] extern crate cpython;
/// # fn main() {
/// use crowbar::{Value, LambdaContext, LambdaResult};
///
/// fn my_handler(event: Value, context: LambdaContext) -> LambdaResult {
///     println!("hello!");
///     Ok(event)
/// }
///
/// lambda!(my_handler);
/// # }
/// ```
macro_rules! lambda {
    (@module ($module:ident, $py2:ident, $py3:ident) @handlers ($($handler:expr => $target:expr),*)) => {
        py_module_initializer!($module, $py2, $py3, |py, m| {
            $(
            m.add(py, $handler, py_fn!(py, x(event: $crate::PyObject, context: $crate::PyObject) -> $crate::PyResult<$crate::PyObject> {
                $crate::handler(py, $target, event, context)
            }))?;
            )*
            Ok(())
        });
    };

    (crate $module:tt { $($handler:expr => $target:expr),* }) => {
        lambda! { @module $module @handlers ($($handler => $target),*) }
    };

    (crate $module:tt { $($handler:expr => $target:expr,)* }) => {
        lambda! { @module $module @handlers ($($handler => $target),*) }
    };

    ($($handler:expr => $target:expr),*) => {
        lambda! { @module (liblambda, initliblambda, PyInit_liblambda) @handlers ($($handler => $target),*) }
    };

    ($($handler:expr => $target:expr,)*) => {
        lambda! { $($handler => $target),* }
    };

    ($f:expr) => {
        lambda! { "handler" => $f, }
    };
}
