//! `auto` rotation (§10): the camera reports, the host decides.

/// Rotation to apply (clockwise, degrees) and whether to mirror horizontally, as Android does.
///
/// - back camera: `(sensor_orientation - device_rotation + 360) % 360`;
/// - front camera: `(sensor_orientation + device_rotation) % 360`, then mirror.
pub fn auto_rotation(sensor_orientation: u16, front: bool, device_rotation: u16) -> (u16, bool) {
    let s = i32::from(sensor_orientation % 360);
    let d = i32::from(device_rotation % 360);
    if front {
        (((s + d) % 360) as u16, true)
    } else {
        (((s - d + 360) % 360) as u16, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_phone() {
        // Back sensor mounted at 90°: phone upright needs 90°, landscape-left needs 0°.
        assert_eq!(auto_rotation(90, false, 0), (90, false));
        assert_eq!(auto_rotation(90, false, 90), (0, false));
        assert_eq!(auto_rotation(90, false, 270), (180, false));
        // Front sensor at 270°.
        assert_eq!(auto_rotation(270, true, 0), (270, true));
        assert_eq!(auto_rotation(270, true, 90), (0, true));
    }
}
