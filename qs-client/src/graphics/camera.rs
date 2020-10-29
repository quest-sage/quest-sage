use std::cell::Cell;

use cgmath::{prelude::*, ortho, Matrix4, Point2};

/// The Z axis is expected to be in range 0.0 to 1.0, not -1.0 to 1.0.
/// Multiplying on the left by this matrix converts OpenGL style matrices into `wgpu` style matrices.
#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

pub enum CameraData {
    Orthographic {
        /// Where is the eye in 2D space?
        eye: Point2<f32>,
        /// How many pixels high is the view area?
        view_height: f32,
        /// What is the width/height of the render area?
        aspect_ratio: f32,
    },
}

impl CameraData {
    pub fn generate_projection_matrix(&self) -> Matrix4<f32> {
        match &self {
            CameraData::Orthographic {
                view_height,
                aspect_ratio,
                ..
            } => {
                let width = aspect_ratio * view_height;
                let half_width = 0.5 * width;
                let half_height = 0.5 * view_height;
                let near = -1000.0;
                let far = 1000.0;
                OPENGL_TO_WGPU_MATRIX
                    * ortho(
                        -half_width,
                        half_width,
                        -half_height,
                        half_height,
                        near,
                        far,
                    )
            }
        }
    }

    pub fn generate_view_matrix(&self) -> Matrix4<f32> {
        match &self {
            CameraData::Orthographic { eye, .. } => Matrix4::from_translation(eye.to_vec().extend(0.0)),
        }
    }
}

pub struct Camera {
    data: CameraData,

    /// Caches the value of the camera's projection matrix.
    projection_matrix: Cell<Option<Matrix4<f32>>>,
    /// Caches the value of the camera's view matrix.
    view_matrix: Cell<Option<Matrix4<f32>>>,
}

impl Camera {
    pub fn new(data: CameraData) -> Camera {
        Camera {
            data,

            projection_matrix: Cell::new(None),
            view_matrix: Cell::new(None),
        }
    }

    pub fn get_projection_matrix(&self) -> Matrix4<f32> {
        let value = self.projection_matrix.get();
        match value {
            Some(matrix) => matrix,
            None => {
                let new_matrix = self.data.generate_projection_matrix();
                self.projection_matrix.set(Some(new_matrix));
                new_matrix
            }
        }
    }

    pub fn get_view_matrix(&self) -> Matrix4<f32> {
        let value = self.view_matrix.get();
        match value {
            Some(matrix) => matrix,
            None => {
                let new_matrix = self.data.generate_view_matrix();
                self.view_matrix.set(Some(new_matrix));
                new_matrix
            }
        }
    }

    pub fn get_data(&self) -> &CameraData {
        &self.data
    }

    /// Deletes all the caches for known matrices.
    pub fn get_data_mut(&mut self) -> &mut CameraData {
        self.projection_matrix.set(None);
        self.view_matrix.set(None);
        &mut self.data
    }
}
