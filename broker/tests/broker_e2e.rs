use skylinebench::bridge_client::BridgeClient;
use skylinebench::contract::Position;
use skylinebench::mock;
use skylinebench::service::{self, BuildRoadArgs, ControlTimeArgs, GetMetricsArgs};

#[tokio::test]
async fn full_observe_build_step_observe_loop() {
    let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
    tokio::spawn(server);
    let client = BridgeClient::new(format!("http://{addr}"));

    // Observe: empty city, full flow.
    let before = service::get_metrics(
        &client,
        GetMetricsArgs {
            groups: vec!["traffic".into()],
        },
    )
    .await
    .unwrap();
    let flow_before = before["traffic"]["flow_percent"].as_f64().unwrap();

    // Act: build a road.
    let built = service::build_road(
        &client,
        BuildRoadArgs {
            from: Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            to: Position {
                x: 50.0,
                y: 0.0,
                z: 0.0,
            },
            road_type: "road".into(),
            snap: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(built["ok"], true);

    // Step the clock.
    let clock = service::control_time(
        &client,
        ControlTimeArgs {
            op: "step".into(),
            ticks: Some(256),
            speed: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(clock["tick"], 256);

    // Observe again: the metric changed (proof the loop is real).
    let after = service::get_metrics(
        &client,
        GetMetricsArgs {
            groups: vec!["traffic".into()],
        },
    )
    .await
    .unwrap();
    let flow_after = after["traffic"]["flow_percent"].as_f64().unwrap();
    assert!(
        flow_after < flow_before,
        "building a road should change traffic flow in the mock"
    );

    // The built segment is observable.
    let obs = service::observe_area(&client, service::ObserveAreaArgs { bounds: None }).await.unwrap();
    assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 1);
}
