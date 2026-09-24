use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Destination {
    Local(PathBuf),
    Remote { session_id: u64, path: String },
}

#[derive(Debug, Clone)]
pub struct TransferJob {
    pub id: u64,
    pub session_id: u64,
    pub direction: Direction,
    pub local_path: PathBuf,
    pub remote_path: String,
    pub display_name: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub status: JobStatus,
    pub attempts: u32,
    pub batch_id: Option<u64>,
    pub resume: bool,
}

impl TransferJob {
    pub fn part_destination(&self) -> Destination {
        match self.direction {
            Direction::Upload => {
                Destination::Remote { session_id: self.session_id, path: format!("{}.part", self.remote_path) }
            }
            Direction::Download => {
                let mut part = self.local_path.clone().into_os_string();
                part.push(".part");
                Destination::Local(PathBuf::from(part))
            }
        }
    }

    pub fn destination(&self) -> Destination {
        match self.direction {
            Direction::Upload => Destination::Remote { session_id: self.session_id, path: self.remote_path.clone() },
            Direction::Download => Destination::Local(self.local_path.clone()),
        }
    }

    pub fn progress_percent(&self) -> u8 {
        if self.total_bytes == 0 {
            return 100;
        }
        ((self.transferred_bytes as f64 / self.total_bytes as f64) * 100.0) as u8
    }
}
