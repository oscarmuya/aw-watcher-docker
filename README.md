# aw-watcher-docker


[![CI](https://github.com/oscarmuya/aw-watcher-docker/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/oscarokello2002/aw-watcher-docker/actions/workflows/ci.yml)
[![Release](https://github.com/oscarmuya/aw-watcher-docker/actions/workflows/release.yml/badge.svg)](https://github.com/oscarokello2002/aw-watcher-docker/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/oscarmuya/aw-watcher-docker?sort=semver)](https://github.com/oscarokello2002/aw-watcher-docker/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/oscarmuya/aw-watcher-docker/total)](https://github.com/oscarokello2002/aw-watcher-docker/releases)

An [ActivityWatch](https://activitywatch.net/) watcher that tracks Docker container activity including runtime, CPU usage, and memory consumption.

## What it tracks

Each running container gets its own AW bucket (`aw-watcher-docker_<container_name>_<hostname>`). Events contain:

| Field | Description |
|---|---|
| `container_name` | Container name (without leading `/`) |
| `container_id` | Short 12-char container ID |
| `image` | Docker image name |
| `status` | Docker status string (e.g. `Up 2 hours`) |
| `cpu_percent` | CPU usage % across all cores |
| `mem_usage_mb` | Current memory usage in MB |
| `mem_limit_mb` | Container memory limit in MB |

## Requirements

- ActivityWatch running on `localhost:5600`
- Docker daemon accessible (user must be in the `docker` group)
- Rust toolchain

## Build

```bash
cargo build --release
```

Binary will be at `target/release/aw-watcher-docker`.

## Run

```bash
# Default: poll every 5s with stats collection
./target/release/aw-watcher-docker

# Custom poll interval, no stats (faster, less overhead)
./target/release/aw-watcher-docker --poll-time 10 --collect-stats false

# Custom AW server
./target/release/aw-watcher-docker --host localhost --port 5600
```

## Autostart with Hyprland

Add to your `~/.config/hypr/hyprland.conf`:

```ini
exec-once = /path/to/aw-watcher-docker
```


## AW Query examples

Bucket names are generated as:

```text
aw-watcher-docker_<container_name>_<hostname>
```

Use `find_bucket("aw-watcher-docker_<container_name>_")` when you do not want to hardcode the hostname.

### List all buckets

```python
RETURN = query_bucket_names();
```

### Get raw events for one container

```python
events = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
RETURN = sort_by_timestamp(events);
```

### Total runtime for one container

```python
events = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
RETURN = sum_durations(events);
```

### Runtime grouped by container

```python
postgres = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
redis = query_bucket(find_bucket("aw-watcher-docker_redis_"));
nginx = query_bucket(find_bucket("aw-watcher-docker_nginx_"));

events = concat(postgres, redis, nginx);
events = merge_events_by_keys(events, ["container_name"]);

RETURN = sort_by_duration(events);
```

### Top containers by runtime

```python
postgres = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
redis = query_bucket(find_bucket("aw-watcher-docker_redis_"));
nginx = query_bucket(find_bucket("aw-watcher-docker_nginx_"));

events = concat(postgres, redis, nginx);
events = merge_events_by_keys(events, ["container_name"]);
events = sort_by_duration(events);

RETURN = limit_events(events, 10);
```

### Runtime grouped by Docker image

```python
postgres = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
redis = query_bucket(find_bucket("aw-watcher-docker_redis_"));
nginx = query_bucket(find_bucket("aw-watcher-docker_nginx_"));

events = concat(postgres, redis, nginx);
events = merge_events_by_keys(events, ["image"]);

RETURN = sort_by_duration(events);
```

### Only containers using a specific image

```python
postgres = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
redis = query_bucket(find_bucket("aw-watcher-docker_redis_"));
nginx = query_bucket(find_bucket("aw-watcher-docker_nginx_"));

events = concat(postgres, redis, nginx);
events = filter_keyvals_regex(events, "image", "^postgres");

RETURN = sort_by_duration(events);
```

### Only currently running container events

```python
events = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
events = filter_keyvals_regex(events, "status", "^Up");

RETURN = sum_durations(events);
```

### Runtime while user was not AFK

```python
docker_events = query_bucket(find_bucket("aw-watcher-docker_postgres_"));

afk_events = query_bucket(find_bucket("aw-watcher-afk_"));
not_afk = filter_keyvals(afk_events, "status", ["not-afk"]);

active_docker_events = filter_period_intersect(docker_events, not_afk);

RETURN = sum_durations(active_docker_events);
```

### Runtime per container while user was not AFK

```python
postgres = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
redis = query_bucket(find_bucket("aw-watcher-docker_redis_"));
nginx = query_bucket(find_bucket("aw-watcher-docker_nginx_"));

docker_events = concat(postgres, redis, nginx);

afk_events = query_bucket(find_bucket("aw-watcher-afk_"));
not_afk = filter_keyvals(afk_events, "status", ["not-afk"]);

docker_events = filter_period_intersect(docker_events, not_afk);
docker_events = merge_events_by_keys(docker_events, ["container_name"]);

RETURN = sort_by_duration(docker_events);
```

### Inspect recent-looking container samples

```python
events = query_bucket(find_bucket("aw-watcher-docker_postgres_"));
events = sort_by_timestamp(events);

RETURN = limit_events(events, 50);
```

