use crate::contract::{ActionError, Position};
use crate::geometry::{horizontal_distance, in_bounds, playable_bounds, MAX_SEGMENT_LENGTH_M};

/// Validate a proposed straight road segment. `known_road_types` is the set the
/// mod reported via `GET /road-types`. Returns the first failing reason, or
/// `Ok(())` if the build is structurally acceptable (the game may still reject
/// it for COLLISION / INSUFFICIENT_FUNDS, which only the mod can know).
pub fn validate_build_road(
    start: Position,
    end: Position,
    road_type: &str,
    known_road_types: &[String],
) -> Result<(), ActionError> {
    if !known_road_types.iter().any(|t| t == road_type) {
        return Err(ActionError::InvalidPrefab);
    }
    let bounds = playable_bounds();
    if !in_bounds(start, bounds) || !in_bounds(end, bounds) {
        return Err(ActionError::OutOfBounds);
    }
    let length = horizontal_distance(start, end);
    if length < f32::EPSILON {
        return Err(ActionError::DegenerateSegment);
    }
    if length > MAX_SEGMENT_LENGTH_M {
        return Err(ActionError::SegmentTooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    fn road_types() -> Vec<String> {
        vec!["road".into(), "highway".into()]
    }

    #[test]
    fn accepts_a_valid_segment() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(50.0, 0.0), "road", &road_types()),
            Ok(())
        );
    }

    #[test]
    fn rejects_unknown_prefab() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(50.0, 0.0), "monorail", &road_types()),
            Err(ActionError::InvalidPrefab)
        );
    }

    #[test]
    fn rejects_out_of_bounds() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(99999.0, 0.0), "road", &road_types()),
            Err(ActionError::OutOfBounds)
        );
    }

    #[test]
    fn rejects_degenerate_segment() {
        assert_eq!(
            validate_build_road(pos(10.0, 10.0), pos(10.0, 10.0), "road", &road_types()),
            Err(ActionError::DegenerateSegment)
        );
    }

    #[test]
    fn rejects_too_long_segment() {
        assert_eq!(
            validate_build_road(
                pos(0.0, 0.0),
                pos(MAX_SEGMENT_LENGTH_M + 1.0, 0.0),
                "road",
                &road_types()
            ),
            Err(ActionError::SegmentTooLong)
        );
    }
}
