//! Reach a service that only listens on the host.
//!
//! A Postgres published with `-p 127.0.0.1:5432:5432` is not reachable from any
//! bridge container: the packet arrives on the bridge interface and the socket
//! only listens on loopback. Only a process inside the host's network namespace
//! can connect to it.
//!
//! So the harness runs one fixed relay there: `socat`, with one rule per declared
//! service, forwarding `127.0.0.1:<port>` to a per-session bridge. The rules come
//! from the resolved settings, never from a command the model wrote.
//!
//! The relay binds the bridge's *gateway* address, not `0.0.0.0`. Binding every
//! interface would publish the user's database to the local network.

use bollard::models::{ContainerCreateBody, HostConfig, NetworkCreateRequest};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
};
use bollard::Docker;

use super::config::{relay_port, Config, Service};
use super::host;
use super::SandboxError;

/// The image carrying `socat`. Kept separate from the agent image so the two can
/// be updated independently.
pub const RELAY_IMAGE: &str = "robi/relay:1";

/// A running relay, and where the sandbox dials it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    pub name: String,
    pub network: String,
    /// The bridge gateway address. This is what the sandbox connects to.
    pub host: String,
}

/// The `socat` command line for the declared services.
///
/// One `TCP-LISTEN` per service, each bound to `gateway` and forwarded to the
/// host's loopback. `fork` so concurrent connections work, `reuseaddr` so a
/// restart does not wait out `TIME_WAIT`.
pub fn rules(services: &[Service], gateway: &str) -> String {
    services
        .iter()
        .map(|service| {
            format!(
                "socat TCP-LISTEN:{},bind={gateway},fork,reuseaddr TCP:127.0.0.1:{}",
                relay_port(service.port),
                service.port
            )
        })
        .collect::<Vec<_>>()
        .join(" & ")
}

/// Ensures the session's relay is running when the workspace needs one.
///
/// Returns `None` when the workspace declared no services, or when it declared a
/// `service_network`: those are reached by name, with no host port involved.
pub async fn ensure(
    docker: &Docker,
    session: &str,
    config: &Config,
) -> Result<Option<Relay>, SandboxError> {
    if !config.uses_relay() {
        return Ok(None);
    }
    let name = host::relay_name(session);
    let network = host::relay_network_name(session);

    let gateway = gateway_of(docker, &network).await?;
    let rules = rules(&config.services, &gateway);

    // Recreate rather than reuse: the rule set changes with the settings, and a
    // stale relay silently forwards the wrong port.
    remove(docker, &name).await;
    docker
        .create_container(
            Some(CreateContainerOptionsBuilder::default().name(&name).build()),
            ContainerCreateBody {
                image: Some(RELAY_IMAGE.to_owned()),
                cmd: Some(vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    // Every rule in the background, then block: the container
                    // lives as long as the shell does.
                    format!("{rules} & wait"),
                ]),
                host_config: Some(HostConfig {
                    // The point of the relay: reach the host's loopback.
                    network_mode: Some("host".to_owned()),
                    cap_drop: Some(vec!["ALL".to_owned()]),
                    security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
                    restart_policy: None,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?;
    docker
        .start_container(&name, None::<StartContainerOptions>)
        .await?;

    Ok(Some(Relay {
        name,
        network,
        host: gateway,
    }))
}

/// The session's bridge, created on first use.
///
/// `internal: true` so the sandbox cannot route out of it: it reaches the relay
/// on-link and nothing else. An internal network still has a gateway address,
/// which is what the relay binds.
async fn gateway_of(docker: &Docker, network: &str) -> Result<String, SandboxError> {
    if docker.inspect_network(network, None).await.is_err() {
        docker
            .create_network(NetworkCreateRequest {
                name: network.to_owned(),
                driver: Some("bridge".to_owned()),
                internal: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|err| SandboxError::Setup(format!("create network {network}: {err}")))?;
    }
    let inspected = docker
        .inspect_network(network, None)
        .await
        .map_err(|err| SandboxError::Setup(format!("inspect network {network}: {err}")))?;
    inspected
        .ipam
        .and_then(|ipam| ipam.config)
        .and_then(|config| config.into_iter().find_map(|entry| entry.gateway))
        .ok_or_else(|| {
            SandboxError::Setup(format!(
                "network {network} has no gateway; the relay cannot bind a safe address"
            ))
        })
}

/// Removes the relay container and its network. Absent is success.
pub async fn remove_all(docker: &Docker, session: &str) {
    remove(docker, &host::relay_name(session)).await;
    let network = host::relay_network_name(session);
    let _ = docker.remove_network(&network).await;
}

async fn remove(docker: &Docker, name: &str) {
    let _ = docker
        .remove_container(
            name,
            Some(RemoveContainerOptionsBuilder::default().force(true).build()),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, port: u16) -> Service {
        Service {
            name: name.to_owned(),
            port,
        }
    }

    #[test]
    fn one_rule_per_service_listening_on_the_offset_port() {
        let rules = rules(&[service("postgres", 5432)], "172.19.0.1");
        assert_eq!(
            rules,
            "socat TCP-LISTEN:15432,bind=172.19.0.1,fork,reuseaddr TCP:127.0.0.1:5432"
        );
    }

    #[test]
    fn the_rule_forwards_to_the_hosts_loopback() {
        let rules = rules(&[service("postgres", 5432)], "172.19.0.1");
        // This is the whole point: the right-hand side must be the host's own
        // loopback, which is why the relay runs in the host namespace.
        assert!(rules.contains("TCP:127.0.0.1:5432"));
    }

    #[test]
    fn the_rule_never_binds_every_interface() {
        let rules = rules(&[service("postgres", 5432)], "172.19.0.1");
        // Binding 0.0.0.0 would publish the user's database to the LAN.
        assert!(!rules.contains("0.0.0.0"));
        assert!(rules.contains("bind=172.19.0.1"));
    }

    #[test]
    fn several_services_become_several_rules() {
        let rules = rules(
            &[service("postgres", 5432), service("redis", 6379)],
            "172.19.0.1",
        );
        assert_eq!(rules.matches("socat ").count(), 2);
        assert!(rules.contains("TCP-LISTEN:16379"));
    }

    #[test]
    fn no_services_is_no_rules() {
        assert_eq!(rules(&[], "172.19.0.1"), "");
    }
}
