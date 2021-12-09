// mdfinfo module

//! This module is reading the mdf file blocks (metadata)

use std::collections::HashSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read};
use std::str;
use std::{collections::HashMap, sync::Arc};
use binread::{BinRead, BinReaderExt};

pub mod mdfinfo3;
pub mod mdfinfo4;

use binread::io::Cursor;
use dashmap::DashMap;
use mdfinfo3::{hd3_comment_parser, hd3_parser, parse_dg3, MdfInfo3, SharableBlocks3};
use mdfinfo4::{
    build_channel_db, extract_xml, hd4_comment_parser, hd4_parser, parse_at4, parse_at4_comments,
    parse_dg4, parse_ev4, parse_ev4_comments, parse_fh, ChannelId, MdfInfo4,
    SharableBlocks,
};


use crate::mdfreader::channel_data::ChannelData;

/// joins mdf versions 3.x and 4.x
#[derive(Debug)]
pub enum MdfInfo {
    V3(Box<MdfInfo3>), // version 3.x
    V4(Box<MdfInfo4>), // version 4.x
}

/// Common Id block structure for both versions 2 and 3
#[derive(Debug, PartialEq, Default, BinRead, Clone)]
pub struct IdBlock {
    id_file_id: [u8; 8],              // "MDF
    id_vers: [u8; 8],                 // version in char
    id_prog: [u8; 8],                 // logger id
    pub id_default_byteorder: u16,    // 0 Little endian, >= 1 Big endian, only valid for 3.x
    id_floatingpointformat: u16,      // default floating point number. 0: IEEE754, 1: G_Float, 2: D_Float, only valid for 3.x
    pub id_ver: u16,                  // version number, valid for both 3.x and 4.x
    id_check: [u8; 2],                // check
    id_reserved: [u8; 26],
    id_unfin_flags: u16,              // only valid for 4.x but can exist in 3.x
    id_custom_unfin_flags: u16,       // only valid for 4.x but can exist in 3.x
}

/// implements MdfInfo creation and manipulation functions
impl MdfInfo {
    /// creates new MdfInfo from file
    pub fn new(file_name: &str) -> MdfInfo {
        let f: File = OpenOptions::new()
            .read(true)
            .write(false)
            .open(file_name)
            .expect("Cannot find the file");
        let mut rdr = BufReader::new(&f);
        // Read beginning of ID Block
        let mut buf = [0u8; 64]; // reserved
        rdr.read_exact(&mut buf).unwrap();
        let mut block = Cursor::new(buf);
        let id: IdBlock = block.read_le().unwrap();
        let mdf_info: MdfInfo;
        
        // Depending of version different blocks
        if id.id_ver < 400 {
            let mut sharable: SharableBlocks3 = SharableBlocks3 {
                cc: HashMap::new(),
                ce: HashMap::new(),
            };
            // Read HD Block
            let (hd, position) = hd3_parser(&mut rdr, id.id_ver);
            let (hd_comment, position) = hd3_comment_parser(&mut rdr, &hd, position);

            // Read DG Block
            let (mut dg, _, n_cg, n_cn) =
                parse_dg3(&mut rdr, hd.hd_dg_first.into(), position, &mut sharable, id.id_default_byteorder);

            mdf_info = MdfInfo::V3(Box::new(MdfInfo3 {
                file_name: file_name.to_string(),
                id_block: id,
                hd_block: hd,
                hd_comment,
            }));
        } else {
            let mut sharable: SharableBlocks = SharableBlocks {
                md: HashMap::new(),
                tx: Arc::new(DashMap::new()),
                cc: HashMap::new(),
                si: HashMap::new(),
            };
            // Read HD block
            let hd = hd4_parser(&mut rdr);
            let (hd_comment, position) = hd4_comment_parser(&mut rdr, &hd);
            // FH block
            let (fh, position) = parse_fh(&mut rdr, hd.hd_fh_first, position);

            // AT Block read
            let (at, position) = parse_at4(&mut rdr, hd.hd_at_first, position);
            let (c, position) = parse_at4_comments(&mut rdr, &at, position);
            sharable.md.extend(c.into_iter());

            // EV Block read
            let (ev, position) = parse_ev4(&mut rdr, hd.hd_ev_first, position);
            let (c, position) = parse_ev4_comments(&mut rdr, &ev, position);
            sharable.md.extend(c.into_iter());

            // Read DG Block
            let (mut dg, _, n_cg, n_cn) =
                parse_dg4(&mut rdr, hd.hd_dg_first, position, &mut sharable);
            extract_xml(&mut sharable.tx); // extract TX xml tag from text

            // make channel names unique, list channels and create master dictionnary
            let channel_names_set = build_channel_db(&mut dg, &sharable, n_cg, n_cn);
            // println!("{}", db);

            mdf_info = MdfInfo::V4(Box::new(MdfInfo4 {
                file_name: file_name.to_string(),
                id_block: id,
                hd_block: hd,
                hd_comment,
                fh,
                at,
                ev,
                dg,
                sharable,
                channel_names_set,
            }));
        };
        mdf_info
    }
    /// gets the version of mdf file
    pub fn get_version(&mut self) -> u16 {
        match self {
            MdfInfo::V3(mdfinfo3) => mdfinfo3.id_block.id_ver,
            MdfInfo::V4(mdfinfo4) => mdfinfo4.id_block.id_ver,
        }
    }
    /// returns the hashmap with :
    /// key = channel_name,
    /// value = (master_name,
    ///          dg_position,
    ///            (cg.block_position, record_id),
    ///            (cn.block_position, cn_record_position))
    pub fn get_channel_id(&mut self, channel_name: &String) -> Option<&ChannelId> {
        let mut channel_id: Option<&ChannelId> = None;
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                channel_id = mdfinfo4.get_channel_id(channel_name);
            }
        }
        channel_id
    }
    /// returns channel's unit string
    pub fn get_channel_unit(&self, channel_name: &String) -> String {
        let mut unit: String = String::new();
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                unit = mdfinfo4.get_channel_unit(channel_name);
            }
        }
        unit
    }
    /// returns channel's description string
    pub fn get_channel_desc(&self, channel_name: &String) -> String {
        let mut desc: String = String::new();
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                desc = mdfinfo4.get_channel_desc(channel_name);
            }
        }
        desc
    }
    /// returns channel's associated master channel name string
    pub fn get_channel_master(&self, channel_name: &String) -> String {
        let mut master: String = String::new();
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                master = mdfinfo4.get_channel_master(channel_name);
            }
        }
        master
    }
    /// returns channel's associated master channel type string
    /// 0 = None (normal data channels), 1 = Time (seconds), 2 = Angle (radians),
    /// 3 = Distance (meters), 4 = Index (zero-based index values)
    pub fn get_channel_master_type(&self, channel_name: &String) -> u8 {
        let mut master: u8 = 0;
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                master = mdfinfo4.get_channel_master_type(channel_name);
            }
        }
        master
    }
    /// returns a set of all channel names contained in file
    pub fn get_channel_names_set(&self) -> HashSet<String> {
        let channel_list: HashSet<String>;
        match self {
            MdfInfo::V3(_mdfinfo3) => {
                channel_list = HashSet::new();
            }
            MdfInfo::V4(mdfinfo4) => {
                channel_list = mdfinfo4.get_channel_names_set();
            }
        }
        channel_list
    }
    /// returns a dict of master names keys for which values are a set of associated channel names
    pub fn get_master_channel_names_set(&self) -> HashMap<String, HashSet<String>> {
        let channel_master_list: HashMap<String, HashSet<String>>;
        match self {
            MdfInfo::V3(_mdfinfo3) => {
                channel_master_list = HashMap::new();
            }
            MdfInfo::V4(mdfinfo4) => {
                channel_master_list = mdfinfo4.get_master_channel_names_set().clone();
            }
        }
        channel_master_list
    }
    /// load a set of channels data in memory
    pub fn load_channels_data_in_memory(&mut self, channel_names: HashSet<String>) {
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                mdfinfo4.load_channels_data_in_memory(channel_names);
            }
        }
    }
    /// load all channels data in memory
    pub fn load_all_channels_data_in_memory(&mut self) {
        let channel_set = self.get_channel_names_set();
        self.load_channels_data_in_memory(channel_set);
    }
    /// returns channel's data ndarray.
    pub fn get_channel_data<'a>(&'a mut self, channel_name: &'a String) -> Option<&ChannelData> {
        let mut data: Option<&ChannelData> = None;
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                data = mdfinfo4.get_channel_data(channel_name);
            }
        }
        data
    }
    /// Clears all data arrays
    pub fn clear_channel_data_from_memory(&mut self, channel_names: HashSet<String>) {
        match self {
            MdfInfo::V3(_mdfinfo3) => {}
            MdfInfo::V4(mdfinfo4) => {
                mdfinfo4.clear_channel_data_from_memory(channel_names);
            }
        }
    }
}

impl fmt::Display for MdfInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MdfInfo::V3(mdfinfo3) => {
                writeln!(f, "Version : {}", mdfinfo3.id_block.id_ver)?;
                writeln!(f, "Version : {:?}", mdfinfo3.hd_block)
            }
            MdfInfo::V4(mdfinfo4) => {
                writeln!(f, "Version : {}", mdfinfo4.id_block.id_ver)?;
                writeln!(f, "{}\n", mdfinfo4.hd_block)?;
                let comments = &mdfinfo4.hd_comment;
                for c in comments.iter() {
                    writeln!(f, "{} {}", c.0, c.1)?;
                }
                writeln!(f, "\n")
            }
        }
    }
}
