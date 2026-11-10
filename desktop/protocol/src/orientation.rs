//! `auto` rotation (§10): the camera reports, the host decides.

/// Clockwise rotation (degrees) that makes the camera picture upright, as Android computes
/// `JPEG_ORIENTATION`:
///
/// - back camera: `(sensor_orientation - device_rotation + 360) % 360`;
/// - front camera: `(sensor_orientation + device_rotation) % 360`.
///
/// `device_rotation` is counter-clockwise, like `Display.getRotation()`. No mirroring: camera
/// frames are not mirrored, so the result is the true picture, like a real webcam (video-call
/// apps mirror the self-view themselves). Mirroring is a separate user setting.
pub fn auto_rotation(sensor_orientation: u16, front: bool, device_rotation: u16) -> u16 {
    let s = i32::from(sensor_orientation % 360);
    let d = i32::from(device_rotation % 360);
    if front {
        ((s + d) % 360) as u16
    } else {
        ((s - d + 360) % 360) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_phone() {
        // Back sensor mounted at 90°: phone upright needs 90°, top edge on the left needs 0°,
        // top edge on the right needs 180°.
        assert_eq!(auto_rotation(90, false, 0), 90);
        assert_eq!(auto_rotation(90, false, 90), 0);
        assert_eq!(auto_rotation(90, false, 270), 180);
        assert_eq!(auto_rotation(90, false, 180), 270);
        // Front sensor at 270°.
        assert_eq!(auto_rotation(270, true, 0), 270);
        assert_eq!(auto_rotation(270, true, 90), 0);
        assert_eq!(auto_rotation(270, true, 270), 180);
    }
}
