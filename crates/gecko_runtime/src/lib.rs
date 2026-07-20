use gecko_core::math::{self, Mat4, Quat, Vec3};

pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y_deg: f32,
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cy * cp, sp, sy * cp) * self.distance
    }

    pub fn view(&self) -> Mat4 {
        math::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        math::camera::rh::proj::directx::perspective_infinite_reverse(
            self.fov_y_deg.to_radians(),
            aspect.max(0.01),
            0.01,
        )
    }
}

pub struct CubeEntity {
    pub name: String,
    pub position: [f32; 3],
    pub scale: f32,
    pub spin_speed: f32,
    pub color: [f32; 3],
    pub angle: f32,
}

impl CubeEntity {
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            Quat::from_rotation_y(self.angle),
            Vec3::from(self.position),
        )
    }
}

pub struct Scene {
    pub camera: OrbitCamera,
    pub cubes: Vec<CubeEntity>,
    pub selected: Option<usize>,
    pub show_grid: bool,
}

impl Scene {
    pub fn new() -> Self {
        let cube = |name: &str, position: [f32; 3], spin_speed: f32, color: [f32; 3]| CubeEntity {
            name: name.to_string(),
            position,
            scale: 1.0,
            spin_speed,
            color,
            angle: 0.0,
        };

        Self {
            camera: OrbitCamera {
                target: Vec3::new(0.0, 0.5, 0.0),
                distance: 8.0,
                yaw: 0.9,
                pitch: 0.45,
                fov_y_deg: 55.0,
            },
            cubes: vec![
                cube("Cube A", [0.0, 0.5, 0.0], 0.8, [0.90, 0.45, 0.15]),
                cube("Cube B", [-2.5, 0.5, 1.0], -0.5, [0.25, 0.60, 0.90]),
                cube("Cube C", [2.0, 0.5, -1.5], 1.4, [0.40, 0.85, 0.40]),
            ],
            selected: None,
            show_grid: true,
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn update(&mut self, delta_time: f32) {
        for cube in &mut self.cubes {
            cube.angle += cube.spin_speed * delta_time;
        }
    }

    pub fn draw_list(&self) -> Vec<(Mat4, [f32; 3])> {
        self.cubes.iter().map(|c| (c.model_matrix(), c.color)).collect()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}
