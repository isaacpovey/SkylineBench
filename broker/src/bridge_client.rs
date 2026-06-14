use serde::Serialize;

use crate::contract::*;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

pub struct BridgeClient {
    base: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct BuildRoadBody<'a> {
    start: Position,
    end: Position,
    prefab: &'a str,
    snap_to_existing_nodes: bool,
}

#[derive(Serialize)]
struct ClockBody<'a> {
    op: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticks: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<u8>,
}

impl BridgeClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        // The mod's Mono HttpListener closes each connection after responding
        // without advertising `Connection: close`. reqwest would otherwise pool
        // the dead socket and reuse it for the next request — fine for retried
        // idempotent GETs, but a non-idempotent POST (e.g. /clock) fails with
        // "connection closed before message completed". Disabling idle pooling
        // forces a fresh connection per request.
        let http = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .expect("reqwest client builds with default TLS/runtime config");
        BridgeClient {
            base: base_url.into(),
            http,
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, BridgeError> {
        Ok(self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn health(&self) -> Result<Health, BridgeError> {
        self.get_json("/health").await
    }

    pub async fn network(&self) -> Result<Network, BridgeError> {
        self.get_json("/network").await
    }

    pub async fn buildings(&self) -> Result<Buildings, BridgeError> {
        self.get_json("/buildings").await
    }

    pub async fn zones(&self) -> Result<Zones, BridgeError> {
        self.get_json("/zones").await
    }

    pub async fn metrics(&self) -> Result<Metrics, BridgeError> {
        self.get_json("/metrics").await
    }

    pub async fn road_types(&self) -> Result<RoadTypes, BridgeError> {
        self.get_json("/road-types").await
    }

    pub async fn zone_types(&self) -> Result<ZoneTypes, BridgeError> {
        self.get_json("/zone-types").await
    }

    pub async fn build_road(
        &self,
        start: Position,
        end: Position,
        prefab: &str,
        snap: bool,
    ) -> Result<ActionResult, BridgeError> {
        let body = BuildRoadBody {
            start,
            end,
            prefab,
            snap_to_existing_nodes: snap,
        };
        Ok(self
            .http
            .post(format!("{}/action/build-road", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn validate_road(
        &self,
        start: Position,
        end: Position,
        prefab: &str,
    ) -> Result<ActionResult, BridgeError> {
        let body = BuildRoadBody { start, end, prefab, snap_to_existing_nodes: true };
        Ok(self
            .http
            .post(format!("{}/action/validate-road", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn bulldoze(&self, target_type: &str, id: u32) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "target_type": target_type, "id": id });
        Ok(self
            .http
            .post(format!("{}/action/bulldoze", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn upgrade_road(
        &self,
        segment_id: u32,
        prefab: &str,
    ) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "segment_id": segment_id, "prefab": prefab });
        Ok(self
            .http
            .post(format!("{}/action/upgrade-road", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn set_zone(
        &self,
        rect: Bounds,
        zone_type: &str,
    ) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "rect": rect, "zone_type": zone_type });
        Ok(self
            .http
            .post(format!("{}/action/set-zone", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn load_save(&self, save_name: &str) -> Result<LoadResult, BridgeError> {
        let body = serde_json::json!({ "save_name": save_name });
        Ok(self
            .http
            .post(format!("{}/load-save", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn list_saves(&self) -> Result<Saves, BridgeError> {
        self.get_json("/saves").await
    }

    pub async fn screenshot(
        &self,
        x: f32,
        z: f32,
        size: f32,
        yaw: f32,
        pitch: f32,
        info_view: &str,
    ) -> Result<Vec<u8>, BridgeError> {
        let body = serde_json::json!({
            "x": x, "z": z, "size": size,
            "yaw": yaw, "pitch": pitch, "info_view": info_view,
        });
        Ok(self
            .http
            .post(format!("{}/screenshot", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }

    /// Drive a recorded flyby: the mod interpolates the camera along `keyframes`
    /// over `duration_s` while the sim runs, writing numbered PNG frames into
    /// `out_dir`. Blocks for the whole pass, so the timeout is generous.
    pub async fn flyby(
        &self,
        keyframes: &[crate::service::CameraKeyframe],
        duration_s: f32,
        capture_fps: u32,
        out_dir: &str,
    ) -> Result<(), BridgeError> {
        let body = serde_json::json!({
            "keyframes": keyframes,
            "duration_s": duration_s,
            "capture_fps": capture_fps,
            "out_dir": out_dir,
        });
        self.http
            .post(format!("{}/flyby", self.base))
            .json(&body)
            .timeout(std::time::Duration::from_secs(duration_s as u64 + 30))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn clock(
        &self,
        op: &str,
        ticks: Option<u32>,
        speed: Option<u8>,
    ) -> Result<ClockState, BridgeError> {
        let body = ClockBody { op, ticks, speed };
        Ok(self
            .http
            .post(format!("{}/clock", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    async fn start_mock() -> String {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetches_health() {
        let client = BridgeClient::new(start_mock().await);
        let h = client.health().await.unwrap();
        assert!(h.city_loaded);
    }

    #[tokio::test]
    async fn builds_a_road_and_sees_it_in_network() {
        let client = BridgeClient::new(start_mock().await);
        let res = client
            .build_road(
                Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                "road",
                true,
            )
            .await
            .unwrap();
        assert!(res.ok);
        let net = client.network().await.unwrap();
        assert_eq!(net.segments.len(), 1);
        assert_eq!(net.nodes.len(), 2);
    }

    #[tokio::test]
    async fn fetches_screenshot_png_bytes() {
        let client = BridgeClient::new(start_mock().await);
        let png = client.screenshot(0.0, 0.0, 500.0, 0.0, 90.0, "none").await.unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[tokio::test]
    async fn flyby_writes_stub_frames_into_out_dir() {
        let client = BridgeClient::new(start_mock().await);
        let tmp = std::env::temp_dir().join(format!("flyby_test_{}", std::process::id()));
        let keyframe = crate::service::CameraKeyframe {
            x: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 32.0,
            size: 500.0,
        };
        client.flyby(&[keyframe], 1.0, 12, tmp.to_str().unwrap()).await.unwrap();
        assert!(tmp.join("00001.png").exists());
    }

    #[tokio::test]
    async fn rejects_invalid_prefab_with_reason() {
        let client = BridgeClient::new(start_mock().await);
        let res = client
            .build_road(
                Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                "monorail",
                true,
            )
            .await
            .unwrap();
        assert!(!res.ok);
        assert_eq!(res.reason, Some(ActionError::InvalidPrefab));
    }
}
