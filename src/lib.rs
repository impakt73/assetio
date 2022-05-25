use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap::Mmap;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::{fs, io};

struct AssetRecord {
    offset: u64,
    size: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct AssetId(u64);

impl AssetId {
    fn from_str(str: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        str.hash(&mut hasher);
        Self(hasher.finish())
    }
}

enum LibraryDataSource {
    Buffer(Vec<u8>),
    File(Mmap),
}

type AssetHashMap = HashMap<AssetId, AssetRecord>;

struct Library {
    source: LibraryDataSource,
    assets: AssetHashMap,
}

impl Library {
    fn parse_asset_map(mut data: &[u8]) -> AssetHashMap {
        let num_assets = data.read_u64::<LittleEndian>().unwrap();
        let mut asset_map = AssetHashMap::new();
        for _ in 0..num_assets {
            let asset_id = AssetId(data.read_u64::<LittleEndian>().unwrap());

            let asset_offset = data.read_u64::<LittleEndian>().unwrap();
            let asset_size = data.read_u64::<LittleEndian>().unwrap();

            let asset_record = AssetRecord {
                offset: asset_offset,
                size: asset_size,
            };

            asset_map.insert(asset_id, asset_record);
        }

        asset_map
    }

    fn from_buffer(buffer: Vec<u8>) -> Self {
        let assets = Self::parse_asset_map(&buffer);
        Self {
            source: LibraryDataSource::Buffer(buffer),
            assets,
        }
    }

    fn from_file(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file) }?;
        let assets = Self::parse_asset_map(&mmap);
        Ok(Self {
            source: LibraryDataSource::File(mmap),
            assets,
        })
    }

    fn num_assets(&self) -> usize {
        self.assets.len()
    }

    fn data(&self) -> &[u8] {
        match &self.source {
            LibraryDataSource::Buffer(buffer) => buffer,
            LibraryDataSource::File(mmap) => mmap,
        }
    }

    fn find_asset(&self, asset_id: AssetId) -> Option<&[u8]> {
        if let Some(asset_record) = self.assets.get(&asset_id) {
            let asset_data = &self.data()[(asset_record.offset as usize)
                ..(asset_record.offset as usize + asset_record.size as usize)];
            Some(asset_data)
        } else {
            None
        }
    }
}

#[derive(Clone)]
enum AssetSource {
    Buffer(Vec<u8>),
    File(String),
}

fn query_asset_source_size(source: &AssetSource) -> usize {
    match source {
        AssetSource::Buffer(buffer) => buffer.len(),
        AssetSource::File(path) => fs::metadata(path).unwrap().len() as usize,
    }
}

#[derive(Clone)]
struct AssetDescription {
    name: String,
    source: AssetSource,
}

impl AssetDescription {
    fn from_buffer(name: &str, buffer: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            source: AssetSource::Buffer(buffer.to_vec()),
        }
    }

    fn from_file(name: &str, file_path: &String) -> Self {
        Self {
            name: name.to_owned(),
            source: AssetSource::File(file_path.to_owned()),
        }
    }
}

struct Builder {
    assets: HashMap<String, AssetDescription>,
}

impl Builder {
    fn new() -> Self {
        Self {
            assets: HashMap::new(),
        }
    }

    fn insert(&mut self, asset: &AssetDescription) {
        self.assets.insert(asset.name.clone(), asset.clone());
    }

    fn num_assets(&self) -> usize {
        self.assets.len()
    }

    // TODO: Write data to a file instead of going through memory
    fn build(&self) -> Vec<u8> {
        let mut data = Vec::new();

        let mut asset_map = HashMap::<String, (AssetId, AssetRecord)>::new();

        let asset_data_base_offset = 8 + self.assets.len() * 24;
        let mut cur_asset_data_offset = asset_data_base_offset as u64;
        for desc in self.assets.values() {
            let mut hasher = DefaultHasher::new();
            desc.name.hash(&mut hasher);
            let asset_id = AssetId(hasher.finish());

            let asset_record = AssetRecord {
                offset: cur_asset_data_offset,
                size: query_asset_source_size(&desc.source) as u64,
            };

            cur_asset_data_offset += asset_record.size;

            asset_map.insert(desc.name.clone(), (asset_id, asset_record));
        }

        // Write the asset table
        data.write_u64::<LittleEndian>(self.assets.len() as u64)
            .unwrap();
        for desc in self.assets.values() {
            let asset_map_entry = asset_map.get(&desc.name).unwrap();
            data.write_u64::<LittleEndian>(asset_map_entry.0 .0)
                .unwrap();
            data.write_u64::<LittleEndian>(asset_map_entry.1.offset)
                .unwrap();
            data.write_u64::<LittleEndian>(asset_map_entry.1.size)
                .unwrap();
        }

        // Write the asset data
        for desc in self.assets.values() {
            match &desc.source {
                AssetSource::Buffer(buffer) => {
                    data.extend(buffer);
                }
                AssetSource::File(path) => {
                    data.extend(fs::read(path).unwrap());
                }
            }
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_asset_path(filename: &str) -> String {
        format!("{}/testing/{}", env!("CARGO_MANIFEST_DIR"), filename)
    }

    #[test]
    fn test_builder() {
        let mut builder = Builder::new();

        builder.insert(&AssetDescription::from_file(
            "Test0",
            &make_asset_path("test0.txt"),
        ));
        builder.insert(&AssetDescription::from_file(
            "Test1",
            &make_asset_path("test1.txt"),
        ));
        builder.insert(&AssetDescription::from_file(
            "Test2",
            &make_asset_path("test2.txt"),
        ));

        let data = builder.build();
        let library = Library::from_buffer(data);

        assert_eq!(library.num_assets(), 3);

        let test0_asset_data = library.find_asset(AssetId::from_str("Test0")).unwrap();
        let test0_file_data_vec = fs::read(make_asset_path("test0.txt")).unwrap();
        assert_eq!(test0_asset_data, &test0_file_data_vec);

        let test1_asset_data = library.find_asset(AssetId::from_str("Test1")).unwrap();
        let test1_file_data_vec = fs::read(make_asset_path("test1.txt")).unwrap();
        assert_eq!(test1_asset_data, &test1_file_data_vec);

        let test2_asset_data = library.find_asset(AssetId::from_str("Test2")).unwrap();
        let test2_file_data_vec = fs::read(make_asset_path("test2.txt")).unwrap();
        assert_eq!(test2_asset_data, &test2_file_data_vec);
    }
}
