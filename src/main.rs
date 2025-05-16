use make87::{get_requester, resolve_endpoint_name};
use make87_messages::core::Header;
use make87_messages::spatial::translation::{Translation1D, Translation2D};
use make87_messages::google::protobuf::Timestamp;
use ros2_client::ros2::policy::{Deadline, Lifespan};
use ros2_client::ros2::{policy, QosPolicies, QosPolicyBuilder};
use ros2_client::{Context, Name, NodeName, NodeOptions, ServiceMapping, ServiceTypeName};
use ros2_interfaces_rolling::example_interfaces;
use ros2_interfaces_rolling::example_interfaces::srv::AddTwoIntsResponse;
use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;

fn sanitize_and_checksum(input: &str) -> String {
    let prefix = "ros2_";

    // Sanitize the input string
    let mut sanitized = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }

    // Compute checksum
    let mut sum: u64 = 0;
    for b in input.bytes() {
        sum = (sum * 31 + b as u64) % 1_000_000_007;
    }
    let checksum = sum.to_string();

    // Calculate maximum allowed length for the sanitized string
    const MAX_TOTAL_LENGTH: usize = 256;
    let prefix_length = prefix.len();
    let checksum_length = checksum.len();
    let max_sanitized_length = MAX_TOTAL_LENGTH - prefix_length - checksum_length;

    // Truncate sanitized string if necessary
    if sanitized.len() > max_sanitized_length {
        sanitized.truncate(max_sanitized_length);
    }

    // Construct the final string
    format!("{}{}{}", prefix, sanitized, checksum)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    make87::initialize();

    let context = Context::new()?;
    let node_id = format!("make87_{}", Uuid::new_v4().simple());

    let mut node = context.new_node(NodeName::new("/make87", &node_id)?, NodeOptions::new())?;

    let service_qos: QosPolicies = {
        QosPolicyBuilder::new()
            .history(policy::History::KeepLast { depth: 10 })
            .reliability(policy::Reliability::Reliable {
                max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
            })
            .durability(policy::Durability::Volatile)
            .deadline(Deadline(ros2_client::ros2::Duration::INFINITE))
            .lifespan(Lifespan {
                duration: ros2_client::ros2::Duration::INFINITE,
            })
            .liveliness(policy::Liveliness::Automatic {
                lease_duration: ros2_client::ros2::Duration::INFINITE,
            })
            .build()
    };

    let ros_service_name = resolve_endpoint_name("PROVIDER_ENDPOINT")
        .map(|name| sanitize_and_checksum(&name)) // Prefix and replace '.' with '_'
        .ok_or_else(|| "Failed to resolve topic name PROVIDER_ENDPOINT")?;

    let proxy_ros_service = Arc::new(node.create_server::<example_interfaces::srv::AddTwoInts>(
        ServiceMapping::Enhanced,
        &Name::new("/", &ros_service_name)?,
        &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
        service_qos.clone(),
        service_qos,
    )?);

    let make87_endpoint_name = resolve_endpoint_name("REQUESTER_ENDPOINT")
        .ok_or_else(|| "Failed to resolve topic name REQUESTER_ENDPOINT")?;
    let proxy_make87_requester = Arc::new(
        get_requester::<Translation2D, Translation1D>(make87_endpoint_name)
            .ok_or_else(|| "Failed to get publisher for OUTGOING_MESSAGE")?,
    );

    while let Ok((req_id, req)) = proxy_ros_service.async_receive_request().await {
        // Create the request message
        let request_message = Translation2D {
            header: Some(Header {
                timestamp: Timestamp::get_current_time().into(),
                reference_id: 0,
                entity_path: "/".to_string(),
            }),
            x: req.a as f32,
            y: req.b as f32,
        };

        // Attempt to send the request and handle the response
        match proxy_make87_requester.request(&request_message, None) {
            Ok(response) => {
                let response_message = AddTwoIntsResponse {
                    sum: response.x as i64,
                };

                // Attempt to send the response back
                if let Err(e) = proxy_ros_service.send_response(req_id, response_message) {
                    eprintln!("Failed to send response. Error: {:?}", e);
                    // Optionally, break the loop or continue based on the error
                    break; // Exiting the loop due to a response error
                } else {
                    println!("Responded successfully.");
                }
            }
            Err(e) => {
                eprintln!("Failed to send request: {:?}", e);
                // Optionally, break the loop or continue based on the error
                break; // Exiting the loop due to a request error
            }
        }
    }

    Ok(())
}
