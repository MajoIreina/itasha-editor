//! One temporary transform owner shared by 2D and 3D. Persistence is only touched
//! after commit returns a value; preview and cancel never produce a history entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GestureStamp {
    pub session_epoch: String,
    pub base_revision: String,
    pub gesture_id: String,
    pub mapping_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureError {
    Busy,
    Stale,
    Sequence,
    NoGesture,
}

pub struct Gesture<T> {
    stamp: GestureStamp,
    original: T,
    preview: T,
    sequence: u64,
}

pub struct GestureService<T> {
    active: Option<Gesture<T>>,
}

impl<T> Default for GestureService<T> {
    fn default() -> Self {
        Self { active: None }
    }
}

impl<T: Clone + PartialEq> GestureService<T> {
    pub fn begin(&mut self, stamp: GestureStamp, value: T) -> Result<(), GestureError> {
        if self.active.is_some() {
            return Err(GestureError::Busy);
        }
        self.active = Some(Gesture {
            stamp,
            original: value.clone(),
            preview: value,
            sequence: 0,
        });
        Ok(())
    }

    pub fn preview(
        &mut self,
        stamp: &GestureStamp,
        sequence: u64,
        value: T,
    ) -> Result<&T, GestureError> {
        let active = self.active.as_mut().ok_or(GestureError::NoGesture)?;
        if &active.stamp != stamp {
            return Err(GestureError::Stale);
        }
        if sequence <= active.sequence {
            return Err(GestureError::Sequence);
        }
        active.sequence = sequence;
        active.preview = value;
        Ok(&active.preview)
    }

    pub fn commit(
        &mut self,
        stamp: &GestureStamp,
        sequence: u64,
    ) -> Result<Option<T>, GestureError> {
        let active = self.active.as_ref().ok_or(GestureError::NoGesture)?;
        if &active.stamp != stamp {
            return Err(GestureError::Stale);
        }
        if sequence != active.sequence {
            return Err(GestureError::Sequence);
        }
        let active = self.active.take().unwrap();
        Ok((active.original != active.preview).then_some(active.preview))
    }

    pub fn cancel(&mut self, stamp: &GestureStamp) -> Result<T, GestureError> {
        let active = self.active.as_ref().ok_or(GestureError::NoGesture)?;
        if &active.stamp != stamp {
            return Err(GestureError::Stale);
        }
        Ok(self.active.take().unwrap().original)
    }

    /// Revision/session switches always end the previous owner's transform.
    pub fn invalidate(&mut self) -> Option<T> {
        self.active.take().map(|g| g.original)
    }
    pub fn current(&self) -> Option<(&GestureStamp, u64, &T)> {
        self.active
            .as_ref()
            .map(|g| (&g.stamp, g.sequence, &g.preview))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stamp() -> GestureStamp {
        GestureStamp {
            session_epoch: "a".into(),
            base_revision: "3".into(),
            gesture_id: "drag-1".into(),
            mapping_version: 2,
        }
    }
    #[test]
    fn both_views_share_one_owner_and_one_commit() {
        let mut service: GestureService<i32> = GestureService::default();
        let s = stamp();
        service.begin(s.clone(), 10).unwrap();
        assert_eq!(service.begin(s.clone(), 10), Err(GestureError::Busy));
        for sequence in 1..100 {
            service.preview(&s, sequence, 10 + sequence as i32).unwrap();
        }
        assert_eq!(service.commit(&s, 99).unwrap(), Some(109));
        assert_eq!(service.commit(&s, 99), Err(GestureError::NoGesture));
    }
    #[test]
    fn stale_preview_cannot_overwrite_newer_input_and_cancel_restores_original() {
        let mut service: GestureService<i32> = GestureService::default();
        let s = stamp();
        service.begin(s.clone(), 7).unwrap();
        service.preview(&s, 2, 22).unwrap();
        assert_eq!(service.preview(&s, 1, 11), Err(GestureError::Sequence));
        let mut other = s.clone();
        other.session_epoch = "b".into();
        assert_eq!(service.preview(&other, 3, 33), Err(GestureError::Stale));
        assert_eq!(service.cancel(&s), Ok(7));
        assert!(service.current().is_none());
        service.begin(other, 9).unwrap();
        assert_eq!(service.invalidate(), Some(9));
    }
}
