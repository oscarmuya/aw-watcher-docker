use std::collections::HashMap;
use std::time::Duration;

use aw_client_rust::AwClient;
use aw_models::Event;
use bollard::Docker;
use bollard::models::ContainerStatsResponse;
use bollard::query_parameters::{ListContainersOptionsBuilder, StatsOptionsBuilder};
use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use tokio::time::sleep;

/// ActivityWatch watcher for Docker container activity.
/// Tracks running containers, their resource usage, and state changes.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// ActivityWatch server host
    #[arg(long, default_value = "localhost")]
    host: String,

    /// ActivityWatch server port
    #[arg(long, default_value_t = 5600)]
    port: u16,

    /// Poll interval in seconds
    #[arg(long, default_value_t = 5.0)]
    poll_time: f64,

    /// Disable CPU and memory stats collection (slightly more expensive per poll)
    #[arg(long, default_value_t = false)]
    no_collect_stats: bool,
}

impl Args {
    fn collect_stats(&self) -> bool {
        !self.no_collect_stats
    }
}

#[derive(Debug, Clone)]
struct ContainerSnapshot {
    id: String,
    name: String,
    image: String,
    status: String,
    cpu_percent: f64,
    mem_usage_mb: f64,
    mem_limit_mb: f64,
}

fn calc_cpu_percent(stats: &ContainerStatsResponse) -> f64 {
    fn calc(stats: &ContainerStatsResponse) -> Option<f64> {
        let cpu = stats.cpu_stats.as_ref()?;
        let precpu = stats.precpu_stats.as_ref()?;

        let cpu_usage = cpu.cpu_usage.as_ref()?;
        let precpu_usage = precpu.cpu_usage.as_ref()?;

        let cpu_delta = cpu_usage.total_usage? as f64 - precpu_usage.total_usage? as f64;

        let system_delta = cpu.system_cpu_usage? as f64 - precpu.system_cpu_usage? as f64;

        let num_cpus = cpu.online_cpus.unwrap_or(1) as f64;

        if system_delta > 0.0 && cpu_delta >= 0.0 {
            Some((cpu_delta / system_delta) * num_cpus * 100.0)
        } else {
            None
        }
    }

    calc(stats).unwrap_or(0.0)
}

async fn fetch_container_stats(docker: &Docker, container_id: &str) -> (f64, f64, f64) {
    let stats_options = StatsOptionsBuilder::default()
        .stream(false)
        .one_shot(true)
        .build();

    let stream = &mut docker.stats(container_id, Some(stats_options)).take(1);

    if let Some(Ok(stats)) = stream.next().await {
        let cpu = calc_cpu_percent(&stats);

        let (mem_usage, mem_limit) = if let Some(mem) = stats.memory_stats.as_ref().unwrap().usage {
            let limit = stats.memory_stats.as_ref().unwrap().max_usage;

            (
                mem as f64 / 1_048_576.0,
                if let Some(val) = limit {
                    val as f64
                } else {
                    0.0
                } / 1_048_576.0,
            )
        } else {
            (cpu, 0.0)
        };

        (0.0, mem_usage, mem_limit)
    } else {
        (0.0, 0.0, 0.0)
    }
}

async fn list_containers(docker: &Docker, collect_stats: bool) -> Vec<ContainerSnapshot> {
    let list_options = ListContainersOptionsBuilder::default().all(true).build();

    let containers = match docker.list_containers(Some(list_options)).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to list containers: {e}");
            return vec![];
        }
    };

    let mut snapshots = Vec::new();

    for c in containers {
        let id = c.id.unwrap_or_default();

        let name = c
            .names
            .unwrap_or_default()
            .first()
            .cloned()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();

        let image = c.image.unwrap_or_default();
        let status = c.status.unwrap_or_default();

        let (cpu_percent, mem_usage_mb, mem_limit_mb) = if collect_stats && !id.is_empty() {
            fetch_container_stats(docker, &id).await
        } else {
            (0.0, 0.0, 0.0)
        };

        snapshots.push(ContainerSnapshot {
            id,
            name,
            image,
            status,
            cpu_percent,
            mem_usage_mb,
            mem_limit_mb,
        });
    }

    snapshots
}

fn container_to_event(snapshot: &ContainerSnapshot, duration: Duration) -> Event {
    let mut data: Map<String, Value> = Map::new();

    let short_id = snapshot.id.get(..12).unwrap_or(snapshot.id.as_str());

    data.insert("container_name".into(), json!(snapshot.name));
    data.insert("container_id".into(), json!(short_id));
    data.insert("image".into(), json!(snapshot.image));
    data.insert("status".into(), json!(snapshot.status));
    data.insert(
        "cpu_percent".into(),
        json!((snapshot.cpu_percent * 100.0).round() / 100.0),
    );
    data.insert(
        "mem_usage_mb".into(),
        json!((snapshot.mem_usage_mb * 10.0).round() / 10.0),
    );
    data.insert(
        "mem_limit_mb".into(),
        json!((snapshot.mem_limit_mb * 10.0).round() / 10.0),
    );

    let now: DateTime<Utc> = Utc::now();
    let duration_secs = duration.as_secs_f64();

    Event {
        id: None,
        timestamp: now - TimeDelta::seconds(duration_secs as i64),
        duration: chrono::Duration::milliseconds((duration_secs * 1000.0) as i64),
        data,
    }
}

fn bucket_id(client: &AwClient, container_name: &str) -> String {
    format!("aw-watcher-docker_{}_{}", container_name, client.hostname)
}

async fn ensure_bucket(
    client: &AwClient,
    container_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bid = bucket_id(client, container_name);

    match client.get_bucket(&bid).await {
        Ok(_) => Ok(()),

        Err(err) if err.status().is_some_and(|status| status.as_u16() == 404) => {
            client
                .create_bucket_simple(&bid, "app.docker-container")
                .await?;

            log::info!("Created bucket: {bid}");
            Ok(())
        }

        Err(err) => Err(err.into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let aw = AwClient::new(&args.host, args.port, "aw-watcher-docker")
        .expect("Failed to create a client");

    log::info!("Connected to ActivityWatch at {}:{}", args.host, args.port);

    let docker = Docker::connect_with_local_defaults()?;
    log::info!("Connected to Docker daemon");

    let poll = Duration::from_secs_f64(args.poll_time);
    let pulsetime = args.poll_time * 2.0;

    let mut known_buckets: HashMap<String, bool> = HashMap::new();

    log::info!(
        "Polling every {:.1}s (stats collection: {})",
        args.poll_time,
        args.collect_stats()
    );

    loop {
        let snapshots = list_containers(&docker, args.collect_stats()).await;

        if snapshots.is_empty() {
            log::debug!("No containers found");
        }

        for snapshot in &snapshots {
            if snapshot.name.is_empty() {
                continue;
            }

            if !known_buckets.contains_key(&snapshot.name) {
                if let Err(e) = ensure_bucket(&aw, &snapshot.name).await {
                    log::warn!("Failed to create bucket for {}: {e}", snapshot.name);
                    continue;
                }

                known_buckets.insert(snapshot.name.clone(), true);
            }

            let bid = bucket_id(&aw, &snapshot.name);
            let event = container_to_event(snapshot, poll);

            if let Err(e) = aw.heartbeat(&bid, &event, pulsetime).await {
                log::warn!("Heartbeat failed for {}: {e}", snapshot.name);
            } else {
                log::debug!(
                    "{} | cpu: {:.1}% | mem: {:.1}/{:.1} MB",
                    snapshot.name,
                    snapshot.cpu_percent,
                    snapshot.mem_usage_mb,
                    snapshot.mem_limit_mb,
                );
            }
        }

        sleep(poll).await;
    }
}
