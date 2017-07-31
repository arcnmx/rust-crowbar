#[macro_use(lambda)]
extern crate crowbar;
#[macro_use]
extern crate cpython;
extern crate rusoto_core;
extern crate rusoto_ec2;

use crowbar::{Value, Context, LambdaResult};
use rusoto_core::{DefaultCredentialsProvider, Region, default_tls_client};
use rusoto_ec2::{Ec2, Ec2Client, DescribeRegionsRequest};
use std::default::Default;
use std::env;
use std::str::FromStr;

fn list_regions(_: Value, _: Context) -> LambdaResult {
    let provider = DefaultCredentialsProvider::new()?;
    let region_str = env::var("AWS_DEFAULT_REGION")?;
    let client = Ec2Client::new(default_tls_client()?,
                                provider,
                                Region::from_str(&region_str)?);
    let input: DescribeRegionsRequest = Default::default();

    match client.describe_regions(&input)?.regions {
        Some(regions) => {
            let mut v = vec![];
            for region in regions {
                match region.region_name {
                    Some(s) => v.push(Value::String(s)),
                    _ => {}
                }
            }
            Ok(Value::Array(v))
        }
        None => Ok(Value::Array(vec![])),
    }
}

lambda!(list_regions);
