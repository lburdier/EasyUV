//! Petite algèbre vectorielle sans dépendance.

use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        V3 { x, y, z }
    }
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> V3 {
        let l = self.length();
        if l > 1e-300 {
            self / l
        } else {
            V3::ZERO
        }
    }
    /// Vecteur orthogonal quelconque (pour construire un repère).
    pub fn any_orthogonal(self) -> V3 {
        let a = if self.x.abs() < 0.9 { V3::new(1.0, 0.0, 0.0) } else { V3::new(0.0, 1.0, 0.0) };
        self.cross(a).normalized()
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, s: f64) -> V3 {
        V3::new(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        V3::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

impl V2 {
    pub fn new(x: f64, y: f64) -> Self {
        V2 { x, y }
    }
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn rotated(self, c: f64, s: f64) -> V2 {
        V2::new(c * self.x - s * self.y, s * self.x + c * self.y)
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, s: f64) -> V2 {
        V2::new(self.x * s, self.y * s)
    }
}

/// Valeurs singulières (s1 >= s2 >= 0) d'une matrice 2x2 [[a, b], [c, d]].
pub fn singular_values_2x2(a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    let e = (a + d) * 0.5;
    let f = (a - d) * 0.5;
    let g = (c + b) * 0.5;
    let h = (c - b) * 0.5;
    let q = (e * e + h * h).sqrt();
    let r = (f * f + g * g).sqrt();
    (q + r, (q - r).abs())
}

/// Rotation la plus proche (au sens de Frobenius) d'une matrice 2x2 [[a, b], [c, d]].
/// Retourne (cos, sin).
pub fn closest_rotation_2x2(a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    // La partie "similitude" de M est [[e, -h], [h, e]] avec e=(a+d)/2, h=(c-b)/2.
    let e = (a + d) * 0.5;
    let h = (c - b) * 0.5;
    let n = (e * e + h * h).sqrt();
    if n < 1e-300 {
        (1.0, 0.0)
    } else {
        (e / n, h / n)
    }
}
