//! Window geometry, z-order and damage tracking for one user-space scene.

use crate::{Damage, Error, Point, Rect};

pub const MAX_WINDOWS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    pub id: u32,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowHit {
    pub id: u32,
    pub local: Point,
}

/// Geometry owned by the compositor. The list keeps the bottom window first.
pub struct WindowStack {
    windows: [Window; MAX_WINDOWS],
    len: usize,
    damage: Damage,
}

impl WindowStack {
    pub fn new(width: u32, height: u32) -> Result<Self, Error> {
        let empty = Window {
            id: 0,
            bounds: Rect {
                origin: Point { x: 0, y: 0 },
                width: 0,
                height: 0,
            },
        };
        Ok(Self {
            windows: [empty; MAX_WINDOWS],
            len: 0,
            damage: Damage::new(width, height)?,
        })
    }

    /// Windows in draw order, from the lowest layer to the topmost one.
    pub fn windows(&self) -> &[Window] {
        &self.windows[..self.len]
    }

    pub fn damage(&self) -> &Damage {
        &self.damage
    }

    /// Shares the frame's damage with cursor movement and presentation.
    pub fn damage_mut(&mut self) -> &mut Damage {
        &mut self.damage
    }

    /// Adds a window at the top. Dimensions follow the limits of a `Canvas`.
    pub fn insert(&mut self, id: u32, bounds: Rect) -> Result<(), Error> {
        if bounds.width == 0
            || bounds.height == 0
            || bounds.width > i32::MAX as u32
            || bounds.height > i32::MAX as u32
        {
            return Err(Error::InvalidDimensions);
        }
        if self.windows().iter().any(|window| window.id == id) {
            return Err(Error::DuplicateWindow);
        }
        let slot = self
            .windows
            .get_mut(self.len)
            .ok_or(Error::WindowStackFull)?;
        *slot = Window { id, bounds };
        self.len += 1;
        self.damage.record(bounds);
        Ok(())
    }

    /// Moves a window, keeping its size and its place in the stack.
    pub fn move_to(&mut self, id: u32, origin: Point) -> Result<(), Error> {
        let index = self.index_of(id)?;
        let bounds = &mut self.windows[index].bounds;
        if bounds.origin != origin {
            self.damage.record(*bounds);
            bounds.origin = origin;
            self.damage.record(*bounds);
        }
        Ok(())
    }

    /// Raises a window and records the intersections with layers it passes.
    pub fn raise(&mut self, id: u32) -> Result<(), Error> {
        let index = self.index_of(id)?;
        let bounds = self.windows[index].bounds;
        for window in &self.windows[index + 1..self.len] {
            if let Some(rect) = bounds.intersection(window.bounds) {
                self.damage.record(rect);
            }
        }
        self.windows[index..self.len].rotate_left(1);
        Ok(())
    }

    pub fn remove(&mut self, id: u32) -> Result<(), Error> {
        let index = self.index_of(id)?;
        self.damage.record(self.windows[index].bounds);
        self.windows.copy_within(index + 1..self.len, index);
        self.len -= 1;
        Ok(())
    }

    /// Selects the topmost window containing a visible screen point.
    /// Returns window-local coordinates, including for partly clipped windows.
    pub fn hit_test(&self, at: Point) -> Option<WindowHit> {
        let (width, height) = self.damage.dimensions();
        if at.x < 0 || at.y < 0 || at.x as u32 >= width || at.y as u32 >= height {
            return None;
        }
        self.windows().iter().rev().find_map(|window| {
            let x = i64::from(at.x) - i64::from(window.bounds.origin.x);
            let y = i64::from(at.y) - i64::from(window.bounds.origin.y);
            if x < 0
                || y < 0
                || x >= i64::from(window.bounds.width)
                || y >= i64::from(window.bounds.height)
            {
                return None;
            }
            Some(WindowHit {
                id: window.id,
                local: Point {
                    x: x as i32,
                    y: y as i32,
                },
            })
        })
    }

    fn index_of(&self, id: u32) -> Result<usize, Error> {
        self.windows()
            .iter()
            .position(|window| window.id == id)
            .ok_or(Error::UnknownWindow)
    }
}

#[cfg(test)]
mod tests;
