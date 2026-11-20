use crate::models::Settings;

type Matrix3 = [[f32; 3]; 3];

fn multiply(left: Matrix3, right: Matrix3) -> Matrix3 {
    let mut result = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            for index in 0..3 {
                result[row][column] += left[row][index] * right[index][column];
            }
        }
    }
    result
}

pub fn matrix_for(settings: &Settings) -> [[f32; 5]; 5] {
    let saturation = 1.0 + settings.saturation as f32 / 100.0;
    let luminance = [0.213, 0.715, 0.072];
    let mut saturate = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            saturate[row][column] = luminance[row] * (1.0 - saturation);
            if row == column {
                saturate[row][column] += saturation;
            }
        }
    }

    let angle = (settings.hue as f32).to_radians();
    let (sin, cos) = angle.sin_cos();
    let hue_column = [
        [
            0.213 + cos * 0.787 - sin * 0.213,
            0.715 - cos * 0.715 - sin * 0.715,
            0.072 - cos * 0.072 + sin * 0.928,
        ],
        [
            0.213 - cos * 0.213 + sin * 0.143,
            0.715 + cos * 0.285 + sin * 0.140,
            0.072 - cos * 0.072 - sin * 0.283,
        ],
        [
            0.213 - cos * 0.213 - sin * 0.787,
            0.715 - cos * 0.715 + sin * 0.715,
            0.072 + cos * 0.928 + sin * 0.072,
        ],
    ];
    let hue = std::array::from_fn(|row| std::array::from_fn(|column| hue_column[column][row]));

    let contrast = 1.0 + settings.contrast as f32 / 100.0;
    let color = multiply(
        multiply(saturate, hue),
        [
            [contrast, 0.0, 0.0],
            [0.0, contrast, 0.0],
            [0.0, 0.0, contrast],
        ],
    );
    let mut result = [[0.0; 5]; 5];
    for row in 0..3 {
        for column in 0..3 {
            result[row][column] = color[row][column];
        }
    }
    result[3][3] = 1.0;
    for column in 0..3 {
        result[4][column] = settings.brightness as f32 / 100.0 + (1.0 - contrast) / 2.0;
    }
    result[4][4] = 1.0;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform_rgb(matrix: [[f32; 5]; 5], rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|column| {
            (0..3)
                .map(|row| rgb[row] * matrix[row][column])
                .sum::<f32>()
                + matrix[4][column]
        })
    }

    fn assert_identity(matrix: [[f32; 5]; 5]) {
        let identity = [
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(matrix, identity);
    }

    #[test]
    fn neutral_saturation_is_identity() {
        assert_identity(matrix_for(&Settings {
            saturation: 0,
            ..Settings::default()
        }));
    }

    #[test]
    fn neutral_contrast_is_identity() {
        assert_identity(matrix_for(&Settings {
            contrast: 0,
            ..Settings::default()
        }));
    }

    #[test]
    fn neutral_brightness_is_identity() {
        assert_identity(matrix_for(&Settings {
            brightness: 0,
            ..Settings::default()
        }));
    }

    #[test]
    fn neutral_hue_rotation_is_identity() {
        assert_identity(matrix_for(&Settings {
            hue: 0,
            ..Settings::default()
        }));
    }

    #[test]
    fn zero_saturation_maps_each_channel_to_the_same_luminance() {
        let matrix = matrix_for(&Settings {
            saturation: -100,
            ..Settings::default()
        });
        let gray = transform_rgb(matrix, [0.2, 0.7, 0.4]);
        assert!((gray[0] - gray[1]).abs() < 1.0e-6);
        assert!((gray[1] - gray[2]).abs() < 1.0e-6);
        assert!((gray[0] - (0.2 * 0.213 + 0.7 * 0.715 + 0.4 * 0.072)).abs() < 1.0e-6);
    }

    #[test]
    fn contrast_and_brightness_compose_around_midpoint() {
        let matrix = matrix_for(&Settings {
            brightness: 10,
            contrast: 100,
            ..Settings::default()
        });
        let result = transform_rgb(matrix, [0.5, 0.5, 0.5]);
        for channel in result {
            assert!((channel - 0.6).abs() < 1.0e-6);
        }
    }
}
