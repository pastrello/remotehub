//! Lossless logical composition of ordered IronRDP dirty regions.
//!
//! Only a *presentation snapshot* may be coalesced; source DesktopUpdate
//! events MUST ALL be applied. This avoids stale or blank popup menus.
use crate::rdp_frames::{RawFrame, MAX_PIXELS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Rect {
    fn right(self) -> usize {usize::from(self.x)+usize::from(self.width)}
    fn bottom(self) -> usize {usize::from(self.y)+usize::from(self.height)}
    pub fn union(self, other: Self) -> Result<Self,&'static str> {
        let l=usize::from(self.x).min(usize::from(other.x));
        let t=usize::from(self.y).min(usize::from(other.y));
        let right=self.right().max(other.right());
        let bottom=self.bottom().max(other.bottom());
        Ok(Self {
            x:u16::try_from(l).map_err(|_|"X inválido")?,
            y:u16::try_from(t).map_err(|_|"Y inválido")?,
            width:u16::try_from(right-l).map_err(|_|"Largura inválida")?,
            height:u16::try_from(bottom-t).map_err(|_|"Altura inválida")?,
        })
    }
}

#[derive(Default)]
pub struct Compositor {
    pub width:u16,
    pub height:u16,
    pixels:Vec<u32>,
    dirty:Option<Rect>,
}
impl Compositor {
    pub fn apply(&mut self, width:u16, height:u16, patch:Rect, data:&[u32])
        -> Result<(), &'static str>
    {
        let size=usize::from(width).checked_mul(usize::from(height)).ok_or("Dimensões inválidas")?;
        if size==0 || size>MAX_PIXELS ||
           patch.width==0 || patch.height==0 ||
           patch.right()>usize::from(width) || patch.bottom()>usize::from(height) ||
           data.len()!=usize::from(patch.width)*usize::from(patch.height)
        {return Err("Região de atualização inválida");}
        if self.width!=width || self.height!=height {
            self.width=width;
            self.height=height;
            self.pixels=vec![0;size];
            // A screen-size change requires a complete refresh.
            self.mark_full();
        }
        for row in 0..usize::from(patch.height) {
            let dest=(usize::from(patch.y)+row)*usize::from(width)+usize::from(patch.x);
            let src=row*usize::from(patch.width);
            self.pixels[dest..dest+usize::from(patch.width)]
                .copy_from_slice(&data[src..src+usize::from(patch.width)]);
        }
        self.dirty=Some(match self.dirty {
            Some(existing)=>existing.union(patch)?,
            None=>patch,
        });
        Ok(())
    }
    pub fn mark_full(&mut self) {
        if self.width>0 && self.height>0 {
            self.dirty=Some(Rect{x:0,y:0,width:self.width,height:self.height});
        }
    }
    pub fn next_patch(&mut self, sequence:u64) -> Option<RawFrame> {
        let rect=self.dirty.take()?;
        let mut pixels=Vec::with_capacity(usize::from(rect.width)*usize::from(rect.height));
        for row in 0..usize::from(rect.height) {
            let offset=(usize::from(rect.y)+row)*usize::from(self.width)+usize::from(rect.x);
            pixels.extend_from_slice(&self.pixels[offset..offset+usize::from(rect.width)]);
        }
        Some(RawFrame {
            sequence,full_width:self.width,full_height:self.height,
            x:rect.x,y:rect.y,width:rect.width,height:rect.height,pixels
        })
    }
    pub fn restore_dirty(&mut self, patch:Rect) {
        self.dirty=Some(match self.dirty {
            Some(old)=>old.union(patch).expect("Validated rectangle union"),
            None=>patch,
        });
    }
    pub fn has_dirty(&self)->bool {self.dirty.is_some()}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_updates_compose_without_losing_prior_pixels() {
        let mut c=Compositor::default();
        c.apply(4,3,Rect{x:0,y:0,width:4,height:3},&[1;12]).unwrap();
        let first=c.next_patch(1).unwrap();
        assert_eq!(first.pixels,vec![1;12]);
        c.apply(4,3,Rect{x:1,y:1,width:2,height:1},&[2,3]).unwrap();
        c.apply(4,3,Rect{x:2,y:2,width:1,height:1},&[4]).unwrap();
        let p=c.next_patch(2).unwrap();
        assert_eq!((p.x,p.y,p.width,p.height),(1,1,2,2));
        assert_eq!(p.pixels,vec![2,3,1,4]);
    }
    #[test]
    fn restore_region_retains_later_changes() {
        let mut c=Compositor::default();
        c.apply(3,2,Rect{x:0,y:0,width:3,height:2},&[0;6]).unwrap();
        let first=c.next_patch(1).unwrap();
        c.apply(3,2,Rect{x:2,y:1,width:1,height:1},&[77]).unwrap();
        c.restore_dirty(Rect{x:first.x,y:first.y,width:first.width,height:first.height});
        let p=c.next_patch(2).unwrap();
        assert_eq!(p.pixels[5],77);
    }
    #[test]
    fn framebuffer_bounds_fail_safely() {
        let mut c=Compositor::default();
        assert!(c.apply(2,2,Rect{x:1,y:0,width:2,height:2},&[0;4]).is_err());
        assert!(c.apply(2560,1601,Rect{x:0,y:0,width:1,height:1},&[0]).is_err());
        assert!(!c.has_dirty());
    }
}
