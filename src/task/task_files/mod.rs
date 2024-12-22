mod git_https_file;
mod git_ssh_file;
mod http_file;
mod local_file;
mod s3_file;

pub use git_https_file::GitHttpsFile;
pub use git_ssh_file::GitSshFile;
pub use http_file::HttpFile;
pub use local_file::LocalFile;
pub use s3_file::S3File;

pub enum TaskFile {
    GitHttpsFile(GitHttpsFile),
    GitSshFile(GitSshFile),
    HttpFile(HttpFile),
    LocalFile(LocalFile),
    S3File(S3File),
}

trait SourceType {
    fn from_str(s: &str) -> Result<Self, String>
    where
        Self: Sized;
}
