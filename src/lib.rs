use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap::Mmap;
use std::collections::hash_map::{self, DefaultHasher};
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::io::Write;
use std::ops::Deref;
use std::sync::Arc;
use std::{fs, io};

fn align_up(val: u64, align: u64) -> u64 {
    (val + (align - 1)) & !(align - 1)
}

const ASSET_ALIGN_SIZE: u64 = 64;

#[derive(Debug)]
struct FileHeader {
    magic_number: u64,
}

impl FileHeader {
    fn from_stream<T: Read>(stream: &mut T) -> Result<Self, io::Error> {
        let magic_number = stream.read_u64::<LittleEndian>()?;

        Ok(Self { magic_number })
    }

    fn to_stream<T: Write>(&self, stream: &mut T) -> Result<(), io::Error> {
        stream.write_u64::<LittleEndian>(self.magic_number)?;
        Ok(())
    }

    fn get_serialized_size() -> usize {
        8
    }
}

#[derive(Debug)]
struct AssetTableHeader {
    num_assets: u64,
}

impl AssetTableHeader {
    fn from_stream<T: Read>(stream: &mut T) -> Result<Self, io::Error> {
        let num_assets = stream.read_u64::<LittleEndian>()?;

        Ok(Self { num_assets })
    }

    fn to_stream<T: Write>(&self, stream: &mut T) -> Result<(), io::Error> {
        stream.write_u64::<LittleEndian>(self.num_assets)?;
        Ok(())
    }

    fn get_serialized_size() -> usize {
        8
    }
}

#[derive(Debug)]
struct AssetTableEntry {
    id: u64,
    offset: u64,
    size: u64,
}

impl AssetTableEntry {
    fn from_stream<T: Read>(stream: &mut T) -> Result<Self, io::Error> {
        let id = stream.read_u64::<LittleEndian>()?;
        let offset = stream.read_u64::<LittleEndian>()?;
        let size = stream.read_u64::<LittleEndian>()?;

        Ok(Self { id, offset, size })
    }

    fn to_stream<T: Write>(&self, stream: &mut T) -> Result<(), io::Error> {
        stream.write_u64::<LittleEndian>(self.id)?;
        stream.write_u64::<LittleEndian>(self.offset)?;
        stream.write_u64::<LittleEndian>(self.size)?;
        Ok(())
    }

    fn get_serialized_size() -> usize {
        24
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AssetId(u64);

impl AssetId {
    fn from_str(str: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        str.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Default)]
struct AssetTable {
    entries: HashMap<AssetId, AssetTableEntry>,
}

impl AssetTable {
    fn from_stream<T: Read>(mut stream: &mut T) -> Result<Self, io::Error> {
        let header = AssetTableHeader::from_stream(&mut stream)?;
        let mut asset_table = AssetTable::default();
        // TODO: Prevent infinite loops on num_assets and 32/64bit issues with offset/size
        for _ in 0..header.num_assets {
            let entry = AssetTableEntry::from_stream(&mut stream)?;
            // TODO: Perform basic bounds checking here so it doesn't blow up later
            asset_table.entries.insert(AssetId(entry.id), entry);
        }

        Ok(asset_table)
    }
}

pub struct LibraryAssetIterator<'a> {
    inner: hash_map::Values<'a, AssetId, AssetTableEntry>,
}

impl<'a> Iterator for LibraryAssetIterator<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| entry.id)
    }
}

pub struct Library {
    source: Arc<AssetSource>,
    assets: AssetTable,
}

impl Library {
    pub fn new(file: &File) -> Result<Self, io::Error> {
        let source = unsafe { Mmap::map(file) }?;
        let mut data = source.deref();
        let file_header = FileHeader::from_stream(&mut data)?;
        if file_header.magic_number != 0xdeadbeef_u64 {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let assets = AssetTable::from_stream(&mut data)?;
        let source = Arc::new(AssetSource { handle: source });
        Ok(Self { source, assets })
    }

    pub fn assets(&self) -> LibraryAssetIterator {
        LibraryAssetIterator {
            inner: self.assets.entries.values(),
        }
    }
}

#[derive(Clone)]
pub struct AssetDescription {
    path: String,
}

impl AssetDescription {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
        }
    }
}

pub struct Builder {
    assets: HashMap<String, AssetDescription>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
        }
    }

    pub fn insert(&mut self, asset: &AssetDescription) {
        self.assets.insert(asset.path.clone(), asset.clone());
    }

    pub fn build<T: Write>(&self, mut output: &mut T) -> Result<(), io::Error> {
        let mut asset_entries = Vec::new();

        let asset_data_base_offset = (FileHeader::get_serialized_size()
            + AssetTableHeader::get_serialized_size()
            + self.assets.len() * AssetTableEntry::get_serialized_size())
            as u64;

        let aligned_asset_data_base_offset = align_up(asset_data_base_offset, ASSET_ALIGN_SIZE);

        let mut cur_asset_data_offset = aligned_asset_data_base_offset;

        for desc in self.assets.values() {
            let mut hasher = DefaultHasher::new();
            desc.path.hash(&mut hasher);
            let id = hasher.finish();

            let offset = cur_asset_data_offset;
            let size = fs::metadata(&desc.path).unwrap().len();

            // Ensure that new asset offsets always begin at an aligned address
            cur_asset_data_offset += align_up(size, ASSET_ALIGN_SIZE);

            let entry = AssetTableEntry { id, offset, size };

            asset_entries.push(entry);
        }

        // Write the file header
        let file_header = FileHeader {
            magic_number: 0xdeadbeef_u64,
        };
        file_header.to_stream(&mut output)?;

        // Write the asset table header
        let asset_table_header = AssetTableHeader {
            num_assets: asset_entries.len() as u64,
        };
        asset_table_header.to_stream(&mut output)?;

        // Write the asset table entries
        for entry in &asset_entries {
            entry.to_stream(&mut output)?;
        }

        // Write padding bytes before the assets
        let padding_bytes = aligned_asset_data_base_offset - asset_data_base_offset;
        for _ in 0..padding_bytes {
            output.write_u8(0)?;
        }

        // Write the asset data
        for desc in self.assets.values() {
            let mut file = File::open(&desc.path)?;
            let mut bytes_copied = io::copy(&mut file, &mut output)?;

            // Write padding bytes until we hit the required alignment for asset data
            let aligned_size = align_up(bytes_copied, ASSET_ALIGN_SIZE);
            while bytes_copied != aligned_size {
                output.write_u8(0)?;
                bytes_copied += 1;
            }
        }

        Ok(())
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Asset {
    path: String,
    offset: u64,
    size: u64,
    source: Arc<AssetSource>,
}

impl Asset {
    pub fn path(&self) -> &String {
        &self.path
    }

    pub fn data(&self) -> &[u8] {
        &(&self.source.handle)[(self.offset as usize)..((self.offset + self.size) as usize)]
    }
}

struct AssetSource {
    handle: Mmap,
}

pub trait AssetLoader {
    fn load(&self, path: &str) -> Option<Arc<Asset>>;
}

pub struct FileAssetLoader {}

impl FileAssetLoader {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for FileAssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetLoader for FileAssetLoader {
    fn load(&self, path: &str) -> Option<Arc<Asset>> {
        let mut result = None;

        if let Ok(file) = File::open(path) {
            if let Ok(mmap) = unsafe { Mmap::map(&file) } {
                let path = path.to_owned();
                let offset = 0;
                let size = mmap.len() as u64;
                let source = Arc::new(AssetSource { handle: mmap });
                let asset = Arc::new(Asset {
                    path,
                    offset,
                    size,
                    source,
                });

                result = Some(asset);
            }
        }

        result
    }
}

impl AssetLoader for Library {
    fn load(&self, path: &str) -> Option<Arc<Asset>> {
        let mut result = None;

        if let Some(entry) = self.assets.entries.get(&AssetId::from_str(path)) {
            let path = path.to_owned();
            let offset = entry.offset;
            let size = entry.size;
            let asset = Arc::new(Asset {
                path,
                offset,
                size,
                source: self.source.clone(),
            });

            result = Some(asset);
        }

        result
    }
}

pub struct AssetManager<T: AssetLoader> {
    assets: HashMap<String, Arc<Asset>>,
    loader: T,
}

impl<T: AssetLoader> AssetManager<T> {
    pub fn new(loader: T) -> Self {
        Self {
            assets: HashMap::new(),
            loader,
        }
    }

    pub fn load(&mut self, path: &str) -> Option<Arc<Asset>> {
        let mut result = None;
        if let Some(asset) = self.assets.get(path) {
            result = Some(asset.clone());
        } else if let Some(asset) = self.loader.load(path) {
            result = Some(asset.clone());
            self.assets.insert(path.to_owned(), asset);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::io::Seek;

    use super::*;

    fn make_asset_path(filename: &str) -> String {
        format!("{}/testing/{}", env!("CARGO_MANIFEST_DIR"), filename)
    }

    fn get_test_asset_paths() -> Vec<String> {
        vec![
            make_asset_path("test0.txt"),
            make_asset_path("test1.txt"),
            make_asset_path("test2.txt"),
        ]
    }

    #[test]
    fn align() {
        assert_eq!(align_up(0, 4), 0);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(2, 4), 4);
        assert_eq!(align_up(3, 4), 4);
        assert_eq!(align_up(4, 4), 4);

        assert_eq!(align_up(54, 64), 64);
    }

    #[test]
    fn filesystem() {
        let asset_paths = get_test_asset_paths();

        let loader = FileAssetLoader::new();
        let mut mgr = AssetManager::<FileAssetLoader>::new(loader);

        for path in &asset_paths {
            let asset = mgr.load(path).unwrap();
            let file_data = fs::read(path).unwrap();
            assert_eq!(asset.data(), &file_data);
        }
    }

    #[test]
    fn library() -> Result<(), io::Error> {
        let asset_paths = get_test_asset_paths();

        let mut builder = Builder::new();

        for path in &asset_paths {
            builder.insert(&AssetDescription::new(path));
        }

        let mut file = tempfile::tempfile()?;
        builder.build(&mut file)?;
        file.rewind()?;

        let library = Library::new(&file)?;

        let mut mgr = AssetManager::<Library>::new(library);

        for path in &asset_paths {
            let asset = mgr.load(path).unwrap();
            let file_data = fs::read(path).unwrap();
            assert_eq!(asset.data(), &file_data);
        }

        Ok(())
    }
}
