use binread::{BinRead, BinReaderExt};
use byteorder::{LittleEndian, ReadBytesExt};
use chrono::NaiveDate;
use encoding::all::{ASCII, ISO_8859_1};
use encoding::{DecoderTrap, Encoding};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::default::Default;
use std::fs::File;
use std::io::{prelude::*, Cursor};
use std::io::BufReader;

use crate::mdfreader::channel_data::{data_type_init, ChannelData};
use crate::mdfinfo::IdBlock;

#[derive(Debug, Default)]
pub struct MdfInfo3 {
    /// file name string
    pub file_name: String,
    pub id_block: IdBlock,
    pub hd_block: Hd3,
    pub hd_comment: String,
    /// data group block linking channel group/channel/conversion/..etc. and data block
    pub dg: HashMap<u32, Dg3>,
    /// Conversion and CE blocks
    pub sharable: SharableBlocks3,
    /// set of all channel names
    pub channel_names_set: ChannelNamesSet3, // set of channel names
}

pub(crate) type ChannelId3 = (String, u32, (u32, u16), u32);
pub(crate) type ChannelNamesSet3 = HashMap<String, ChannelId3>;

/// MdfInfo3's implementation
impl MdfInfo3 {
    pub fn get_channel_id(&self, channel_name: &String) -> Option<&ChannelId3> {
        self.channel_names_set.get(channel_name)
    }
    /// Returns the channel's unit string. If it does not exist, it is an empty string.
    pub fn get_channel_unit(&self, channel_name: &String) -> String {
        let mut unit: String = String::new();
        if let Some((_master, dg_pos, (_cg_pos, rec_id), cn_pos)) =
            self.get_channel_id(channel_name)
        {
            if let Some(dg) = self.dg.get(dg_pos) {
                if let Some(cg) = dg.cg.get(&rec_id) {
                    if let Some(cn) = cg.cn.get(&cn_pos) {
                        if let Some(array) = self.sharable.cc.get(&cn.block1.cn_cc_conversion) {
                            let txt = array.0.cc_unit;
                            ISO_8859_1
                                .decode_to(&txt, DecoderTrap::Replace, &mut unit)
                                .expect("channel description is latin1 encoded");
                            unit = unit.trim_end_matches(char::from(0)).to_string();
                        } 
                    }
                }
            }
        }
        unit
    }
    /// Returns the channel's description. If it does not exist, it is an empty string
    pub fn get_channel_desc(&self, channel_name: &String) -> String {
        let mut desc = String::new();
        if let Some((_master, dg_pos, (_cg_pos, rec_id), cn_pos)) =
            self.get_channel_id(channel_name)
        {
            if let Some(dg) = self.dg.get(&dg_pos) {
                if let Some(cg) = dg.cg.get(&rec_id) {
                    if let Some(cn) = cg.cn.get(&cn_pos) {
                        desc = cn.description.clone();
                    }
                }
            }
        }
        desc
    }
    /// returns the master channel associated to the input channel name
    pub fn get_channel_master(&self, channel_name: &String) -> String {
        let mut master = String::new();
        if let Some((m, _dg_pos, (_cg_pos, _rec_idd), _cn_pos)) =
            self.get_channel_id(channel_name)
        {
            master = m.clone();
        }
        master
    }
}

/// MDF3 - common Header
#[derive(Debug, BinRead, Default)]
#[br(little)]
pub struct Blockheader3 {
    hdr_id: [u8; 2], // 'XX' Block type identifier
    hdr_len: u16,    // block size
}

pub fn parse_block_header(rdr: &mut BufReader<&File>) -> Blockheader3 {
    let header: Blockheader3 = rdr.read_le().unwrap();
    header
}

/// HD3 strucutre
#[derive(Debug, PartialEq, Default)]
pub struct Hd3 {
    hd_id: [u8; 2],     // HD
    hd_len: u16,        // Length of block in bytes
    pub hd_dg_first: u32,   // Pointer to the first data group block (DGBLOCK) (can be NIL)
    hd_md_comment: u32, // Pointer to the measurement file comment (TXBLOCK) (can be NIL)
    hd_pr: u32,         // Program block

    // Data members
    hd_n_datagroups: u16, // Time stamp in nanoseconds elapsed since 00:00:00 01.01.1970 (UTC time or local time, depending on "local time" flag, see [UTC]).
    hd_date: (u32, u32, i32), // Date at which the recording was started in "DD:MM:YYYY" format
    hd_time: (u32, u32, u32), // Time at which the recording was started in "HH:MM:SS" format
    hd_author: String,    // Author's name
    hd_organization: String, // name of the organization or department
    hd_project: String,   // project name
    hd_subject: String,   // subject or measurement object
    hd_start_time_ns: Option<u64>, // time stamp at which recording was started in nanosecond
    hd_time_offset: Option<i16>, // UTC time offset
    hd_time_quality: Option<u16>, // time quality class
    hd_time_identifier: Option<String>, // timer identification or time source
}

/// HD3 block strucutre
#[derive(Debug, PartialEq, Default, BinRead)]
pub struct Hd3Block {
    hd_id: [u8; 2],     // HD
    hd_len: u16,        // Length of block in bytes
    pub hd_dg_first: u32,   // Pointer to the first data group block (DGBLOCK) (can be NIL)
    hd_md_comment: u32, // Pointer to the measurement file comment (TXBLOCK) (can be NIL)
    hd_pr: u32,         // Program block
    hd_n_datagroups: u16, // number of datagroup
    hd_date: [u8; 10], // Date at which the recording was started in "DD:MM:YYYY" format
    hd_time: [u8; 8], // Time at which the recording was started in "HH:MM:SS" format
    hd_author: [u8; 32],    // Author's name
    hd_organization: [u8; 32], // name of the organization or department
    hd_project: [u8; 32],   // project name
    hd_subject: [u8; 32],   // subject or measurement object
}
#[derive(Debug, PartialEq, Default, BinRead)]
pub struct Hd3Block32 {
    hd_start_time_ns: u64, // time stamp at which recording was started in nanosecond
    hd_time_offset: i16, // UTC time offset
    hd_time_quality: u16, // time quality class
    hd_time_identifier: [u8; 32], // timer identification or time source
}

pub fn hd3_parser(rdr: &mut BufReader<&File>, ver: u16) -> (Hd3, i64) {
    let mut buf = [0u8; 164];
    rdr.read_exact(&mut buf).unwrap();
    let mut block = Cursor::new(buf);
    let block: Hd3Block = block.read_le().unwrap();
    let mut datestr = String::new();
    ASCII
        .decode_to(&block.hd_date, DecoderTrap::Replace, &mut datestr)
        .unwrap();
    let mut dateiter = datestr.split(':');
    let day: u32 = dateiter.next().unwrap().parse::<u32>().unwrap();
    let month: u32 = dateiter.next().unwrap().parse::<u32>().unwrap();
    let year: i32 = dateiter.next().unwrap().parse::<i32>().unwrap();
    let hd_date = (day, month, year);
    let mut timestr = String::new();
    ASCII
        .decode_to(&block.hd_time, DecoderTrap::Replace, &mut timestr)
        .unwrap();
    let mut timeiter = timestr.split(':');
    let hour: u32 = timeiter.next().unwrap().parse::<u32>().unwrap();
    let minute: u32 = timeiter.next().unwrap().parse::<u32>().unwrap();
    let sec: u32 = timeiter.next().unwrap().parse::<u32>().unwrap();
    let hd_time = (hour, minute, sec);
    let mut hd_author = String::new();
    ISO_8859_1
        .decode_to(&block.hd_author, DecoderTrap::Replace, &mut hd_author)
        .unwrap();
    let mut hd_organization = String::new();
    ISO_8859_1
        .decode_to(&block.hd_organization, DecoderTrap::Replace, &mut hd_organization)
        .unwrap();
    let mut hd_project = String::new();
    ISO_8859_1
        .decode_to(&block.hd_project, DecoderTrap::Replace, &mut hd_project)
        .unwrap();
    let mut hd_subject = String::new();
    ISO_8859_1
        .decode_to(&block.hd_subject, DecoderTrap::Replace, &mut hd_subject)
        .unwrap();
    let hd_start_time_ns: Option<u64>;
    let hd_time_offset: Option<i16>;
    let hd_time_quality: Option<u16>;
    let hd_time_identifier: Option<String>;
    let position: i64;
    if ver >= 320 {
        let mut buf = [0u8; 44];
        rdr.read_exact(&mut buf).unwrap();
        let mut block = Cursor::new(buf);
        let block: Hd3Block32 = block.read_le().unwrap();
        let mut ti = String::new();
        ISO_8859_1
            .decode_to(&block.hd_time_identifier, DecoderTrap::Replace, &mut ti)
            .unwrap();
        hd_start_time_ns = Some(block.hd_start_time_ns);
        hd_time_offset = Some(block.hd_time_offset);
        hd_time_quality = Some(block.hd_time_quality);
        hd_time_identifier = Some(ti);
        position = 208 + 64; // position after reading ID and HD
    } else {
        // calculate hd_start_time_ns
        hd_start_time_ns = Some(
            u64::try_from(
                NaiveDate::from_ymd(hd_date.2, hd_date.1, hd_date.0)
                    .and_hms(hd_time.0, hd_time.1, hd_time.2)
                    .timestamp_nanos(),
            )
            .unwrap(),
        );
        hd_time_offset = None;
        hd_time_quality = None;
        hd_time_identifier = None;
        position = 164 + 64; // position after reading ID and HD
    }
    (
        Hd3 {
            hd_id: block.hd_id,
            hd_len: block.hd_len,
            hd_dg_first: block.hd_dg_first,
            hd_md_comment: block.hd_md_comment,
            hd_pr: block.hd_pr,
            hd_n_datagroups: block.hd_n_datagroups,
            hd_date,
            hd_time,
            hd_author,
            hd_organization,
            hd_project,
            hd_subject,
            hd_start_time_ns,
            hd_time_offset,
            hd_time_quality,
            hd_time_identifier,
        },
        position,
    )
}

pub fn hd3_comment_parser(
    rdr: &mut BufReader<&File>,
    hd3_block: &Hd3,
    mut position: i64,
) -> (String, i64) {
    let (_, comment, pos) = parse_tx(
        rdr,
        hd3_block.hd_md_comment, position
    );
    position = pos;
    (comment, position)
}

pub fn parse_tx(rdr: &mut BufReader<&File>, target: u32, position: i64) -> (Blockheader3, String, i64) {
    rdr.seek_relative(target as i64 - position).unwrap();
    let block_header: Blockheader3 = parse_block_header(rdr); // reads header
    
    // reads comment
    let mut comment_raw = vec![0; (block_header.hdr_len - 4) as usize];
    rdr.read_exact(&mut comment_raw).unwrap();
    let mut comment: String = String::new();
    ISO_8859_1
        .decode_to(&comment_raw, DecoderTrap::Replace, &mut comment)
        .expect("Reads comment iso 8859 coded");
    let comment: String = comment.trim_end_matches(char::from(0)).into();
    let position = target as i64 + block_header.hdr_len as i64;
    (block_header, comment, position)
}

#[derive(Debug, BinRead, Clone)]
#[br(little)]
pub struct Dg3Block {
    dg_id: [u8; 2],       // DG
    dg_len: u16,          // Length of block in bytes
    dg_dg_next: u32,      // Pointer to next data group block (DGBLOCK) (can be NIL)
    dg_cg_first: u32,     // Pointer to first channel group block (CGBLOCK) (can be NIL)
    dg_tr: u32,           // Pointer to trigger block
    dg_data: u32, // Pointer to data block (DTBLOCK or DZBLOCK for this block type) or data list block (DLBLOCK of data blocks or its HLBLOCK)  (can be NIL)
    dg_n_cg: u16, // number of channel groups
    dg_n_record_ids: u16, // number of record ids
    // reserved: u32, // reserved
}

pub fn parse_dg3_block(rdr: &mut BufReader<&File>, target: u32, position: i64) -> (Dg3Block, i64) {
    rdr.seek_relative(target as i64 - position).unwrap();
    let mut buf = [0u8; 24];
    rdr.read_exact(&mut buf).unwrap();
    let mut block = Cursor::new(buf);
    let block: Dg3Block = block.read_le().unwrap();
    (block, (target + 24).into())
}

/// Dg3 struct wrapping block, comments and linked CG
#[derive(Debug, Clone)]
pub struct Dg3 {
    pub block: Dg3Block,               // DG Block
    pub cg: HashMap<u16, Cg3>,         // CG Block
}

/// Parser for Dg3 and all linked blocks (cg, cn, cc)
pub fn parse_dg3(
    rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
    default_byte_order: u16,
) -> (HashMap<u32, Dg3>, i64, u16, u16) {
    let mut dg: HashMap<u32, Dg3> = HashMap::new();
    let mut n_cn: u16 = 0;
    let mut n_cg: u16 = 0;
    if target > 0 {
        let (block, pos) = parse_dg3_block(rdr, target, position);
        position = pos;
        let mut next_pointer = block.dg_dg_next;
        let (cg, pos, num_cn) = parse_cg3(
            rdr,
            block.dg_cg_first,
            position,
            sharable,
            block.dg_n_record_ids,
            default_byte_order,
        );
        n_cg += block.dg_n_cg;
        n_cn += num_cn;
        let dg_struct = Dg3 {
            block,
            cg,
        };
        dg.insert(target, dg_struct);
        position = pos;
        while next_pointer > 0 {
            let block_start = next_pointer;
            let (block, pos) = parse_dg3_block(rdr, next_pointer, position);
            next_pointer = block.dg_dg_next;
            position = pos;
            let (cg, pos, num_cn) = parse_cg3(
                rdr,
                block.dg_cg_first,
                position,
                sharable,
                block.dg_n_record_ids,
                default_byte_order,
            );
            n_cg += block.dg_n_cg;
            n_cn += num_cn;
            let dg_struct = Dg3 {
                block,
                cg,
            };
            dg.insert(block_start, dg_struct);
            position = pos;
        }
    }
    (dg, position, n_cg, n_cn)
}

/// Cg3 Channel Group block struct
#[derive(Debug, Copy, Clone, Default, BinRead)]
#[br(little)]
pub struct Cg3Block {
    cg_id: [u8; 2],  // CG
    cg_len: u16,      // Length of block in bytes
    pub cg_cg_next: u32,   // Pointer to next channel group block (CGBLOCK) (can be NIL)
    cg_cn_first: u32, // Pointer to first channel block (CNBLOCK) (NIL allowed)
    cg_comment: u32, // CG comment (TXBLOCK) (can be NIL)
    pub cg_record_id: u16, // Record ID, value of the identifier for a record if the DGBLOCK defines a number of record IDs > 0
    cg_n_channels: u16, // number of channels
    pub cg_data_bytes: u16, // Size of data record in Bytes (without record ID)
    pub cg_cycle_count: u32, // Number of records of this type in the data block
    cg_sr: u32, // Pointer to first sample reduction block (SRBLOCK) (NIL allowed)
}

/// Cg3 (Channel Group) block struct parser with linked comments Source Information in sharable blocks
fn parse_cg3_block(
    rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
    record_id_size: u16,
    default_byte_order: u16,
) -> (Cg3, i64, u16) {
    rdr.seek_relative(target as i64 - position).unwrap(); // change buffer position
    let mut buf = vec![0u8; 30];
    rdr.read_exact(&mut buf).unwrap();
    let mut block = Cursor::new(buf);
    let cg: Cg3Block = block.read_le().unwrap();
    position = target as i64 + 30; 

    // reads CN (and other linked block behind like CC, SI, CA, etc.)
    let (cn, pos, n_cn) = parse_cn3(
        rdr,
        cg.cg_cn_first,
        position,
        sharable,
        record_id_size,
        default_byte_order,
    );
    position = pos;

    let record_length = cg.cg_data_bytes;
    let block_position = target;

    let cg_struct = Cg3 {
        block: cg,
        cn,
        block_position,
        master_channel_name: String::new(),
        channel_names: HashSet::new(),
        record_length,
    };

    (cg_struct, position, n_cn)
}

/// Channel Group struct
/// it contains the related channels structure, a set of channel names, the dedicated master channel name and other helper data.
#[derive(Debug, Clone)]
pub struct Cg3 {
    pub block: Cg3Block,
    pub cn: HashMap<u32, Cn3>, // hashmap of channels
    block_position: u32,
    pub master_channel_name: String,
    pub channel_names: HashSet<String>,
    pub record_length: u16, // record length including recordId
}

/// Cg3 blocks and linked blocks parsing
pub fn parse_cg3(
    rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
    record_id_size: u16,
    default_byte_order: u16,
) -> (HashMap<u16, Cg3>, i64, u16) {
    let mut cg: HashMap<u16, Cg3> = HashMap::new();
    let mut n_cn: u16 = 0;
    if target != 0 {
        let (mut cg_struct, pos, num_cn) =
            parse_cg3_block(rdr, target, position, sharable, record_id_size, default_byte_order);
        position = pos;
        let mut next_pointer = cg_struct.block.cg_cg_next;
        cg_struct.record_length += record_id_size;
        cg.insert(cg_struct.block.cg_record_id, cg_struct);
        n_cn += num_cn;

        while next_pointer != 0 {
            let (mut cg_struct, pos, num_cn) =
                parse_cg3_block(rdr, next_pointer, position, sharable, record_id_size, default_byte_order);
            position = pos;
            cg_struct.record_length += record_id_size;
            next_pointer = cg_struct.block.cg_cg_next;
            cg.insert(cg_struct.block.cg_record_id, cg_struct);
            n_cn += num_cn;
        }
    }
    (cg, position, n_cn)
}

/// Cn3 structure containing block but also unique_name, ndarray data
/// and other attributes frequently needed and computed
#[derive(Debug, Default, Clone)]
pub struct Cn3 {
    pub block1: Cn3Block1,
    pub block2: Cn3Block2,
    /// unique channel name string
    pub unique_name: String,
    /// channel comment
    pub comment: String,
    // channel description
    pub description: String,
    /// beginning position of channel in record
    pub pos_byte_beg: u16,
    /// number of bytes taken by channel in record
    pub n_bytes: u16,
    /// channel data
    pub data: ChannelData,
    /// false = little endian
    pub endian: bool,
    /// True if channel is valid = contains data converted
    pub channel_data_valid: bool,
}

/// creates recursively in the channel group the CN blocks and all its other linked blocks (CC, TX, CE, CD)
pub fn parse_cn3(
    rdr: &mut BufReader<&File>,
    mut target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
    record_id_size: u16,
    default_byte_order: u16,
) -> (HashMap<u32, Cn3>, i64, u16) {
    let mut cn: HashMap<u32, Cn3> = HashMap::new();
    let mut n_cn: u16 = 0;
    if target != 0 {
        let (cn_struct, pos) = parse_cn3_block(
            rdr,
            target,
            position,
            sharable,
            record_id_size,
            default_byte_order, 
        );
        position = pos;
        n_cn += 1;
        let mut next_pointer = cn_struct.block1.cn_cn_next;
        cn.insert(target, cn_struct);

        while next_pointer != 0 {
            let (cn_struct, pos) = parse_cn3_block(
                rdr,
                next_pointer,
                position,
                sharable,
                record_id_size,
                default_byte_order,
            );
            position = pos;
            n_cn += 1;
            target = next_pointer.clone();
            next_pointer = cn_struct.block1.cn_cn_next;
            cn.insert(target, cn_struct);
        }
    }
    (cn, position, n_cn)
}

/// Cn3 Channel block struct, first sub block
#[derive(Debug, PartialEq, Default, Clone, BinRead)]
#[br(little)]
pub struct Cn3Block1 {
    cn_id: [u8; 2],  // CN
    cn_len: u16,      // Length of block in bytes
    cn_cn_next: u32, // Pointer to next channel block (CNBLOCK) (can be NIL)
    pub cn_cc_conversion: u32, // Pointer to the conversion formula (CCBLOCK) (can be NIL)
    cn_ce_source: u32, // Pointer to the source-depending extensions (CEBLOCK) of this signal (can be NIL)
    cn_cd_source: u32, // Pointer to the dependency block (CDBLOCK) of this signal (NIL allowed)
    cn_tx_comment: u32, // Pointer to the channel comment (TXBLOCK) of this signal (NIL allowed)
    pub cn_type: u16,       // Channel type, 0 normal data, 1 time channel
    cn_short_name: [u8; 32], // Short signal name
}
/// Cn3 Channel block struct, second sub block
#[derive(Debug, PartialEq, Default, Clone, BinRead)]
#[br(little)]
pub struct Cn3Block2 {
    pub cn_bit_offset: u16, // Start offset in bits to determine the first bit of the signal in the data record.
    pub cn_bit_count: u16, // Number of bits used to encode the value of this signal in a data record
    pub cn_data_type: u16,  // Channel data type of raw signal value
    cn_valid_range_flags: u16,  // Value range valid flag:
    cn_val_range_min: f64, // Minimum signal value that occurred for this signal (raw value) Only valid if "value range valid" flag (bit 3) is set.
    cn_val_range_max: f64, // Maximum signal value that occurred for this signal (raw value) Only valid if "value range valid" flag (bit 3) is set.
    cn_sampling_rate: f64, // Sampling rate of the signal in sec
    cn_tx_long_name: u32, // Short signal name
    cn_tx_display_name: u32, // Short signal name
    cn_byte_offset: u16, // 
}

fn parse_cn3_block(
    rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
    record_id_size: u16,
    default_byte_order: u16,
) -> (Cn3, i64) {
    rdr.seek_relative(target as i64 - position).unwrap(); // change buffer position
    let mut buf = vec![0u8; 228];
    rdr.read_exact(&mut buf).unwrap();
    position = target as i64 + 228;
    let mut block = Cursor::new(buf);
    let block1: Cn3Block1 = block.read_le().unwrap();
    let mut desc = vec![0u8; 128];
    block.read_exact(&mut desc).unwrap();
    let block2: Cn3Block2 = block.read_le().unwrap();
    let pos_byte_beg = block2.cn_bit_offset / 8 + record_id_size;
    let mut n_bytes = block2.cn_bit_count / 8u16;
    if (block2.cn_bit_count % 8) != 0 {
        n_bytes += 1;
    }
    
    let mut unique_name = String::new();
    ISO_8859_1
        .decode_to(&block1.cn_short_name, DecoderTrap::Replace, &mut unique_name)
        .expect("channel name is latin1 encoded");
    unique_name = unique_name.trim_end_matches(char::from(0)).to_string();
    if block2.cn_tx_long_name !=0 {
        // Reads TX long name
        let (_, name, pos) = parse_tx(rdr, block2.cn_tx_long_name, position);
        unique_name = name;
        position = pos;
    }

    let mut description = String::new();
    ISO_8859_1
        .decode_to(&desc, DecoderTrap::Replace, &mut description)
        .expect("channel description is latin1 encoded");
    description = description.trim_end_matches(char::from(0)).to_string();

    let mut comment = String::new();
    if block1.cn_tx_comment !=0 {
        // Reads TX comment
        let (_, cm, pos) = parse_tx(rdr, block1.cn_tx_comment, position);
        comment = cm;
        position = pos; 
    }

    // Reads CC block
    if !sharable.cc.contains_key(&block1.cn_cc_conversion) {
        position = parse_cc3_block(rdr, block1.cn_cc_conversion, position, sharable);
    }

    // Reads CE block
    if !sharable.ce.contains_key(&block1.cn_ce_source) {
        position = parse_ce(rdr, block1.cn_ce_source, position, sharable);
    }

    let mut endian: bool = false; // Little endian by default
    if block2.cn_data_type >= 13
    {
        endian = false; // little endian
    } else if block2.cn_data_type >=9
    {
        endian = true; // big endian
    } else if block2.cn_data_type <= 3 {
        if default_byte_order == 0 {
            endian = false; // little endian
        } else {
            endian = true; // big endian
        }
    }
    let data_type = convert_data_type_3to4(block2.cn_data_type);

    let cn_struct = Cn3 {
        block1,
        block2,
        description,
        comment,
        unique_name,
        pos_byte_beg,
        n_bytes,
        data: data_type_init(0, data_type, n_bytes as u32, false),
        endian,
        channel_data_valid: false,
    };

    (cn_struct, position)
}

pub fn convert_data_type_3to4(mdf3_datatype:u16) -> u8 {
    match mdf3_datatype {
        0 => 0,
        1 => 2,
        2 => 4,
        3 => 4,
        7 => 6,
        8=>10,
        9=>1,
        10=> 3,
        11=> 5,
        12=> 5,
        13=>0,
        14=>2,
        15=>4,
        16=>4,
        _=>13,
    }
}

/// sharable blocks (most likely referenced multiple times and shared by several blocks)
/// that are in sharable fields and holds CC, CE, TX blocks
#[derive(Debug, Default, Clone)]
pub struct SharableBlocks3 {
    pub(crate) cc: HashMap<u32, (Cc3Block, Conversion)>,
    pub(crate) ce: HashMap<u32, CeBlock>,
}
/// Cc3 Channel conversion block struct, second sub block
#[derive(Debug, Clone, BinRead)]
#[br(little)]
pub struct Cc3Block {
    cc_id: [u8; 2],  // CC
    cc_len: u16,      // Length of block in bytes
    cc_valid_range_flags: u16,  // Physical value range valid flag
    cc_val_range_min: f64, // Minimum physical signal value that occurred for this signal. Only valid if "physical value range valid" flag is set.
    cc_val_range_max: f64, // Maximum physical signal value that occurred for this signal. Only valid if "physical value range valid" flag is set.
    cc_unit: [u8; 20], // physical unit of the signal
    cc_type: u16, // Conversion type
    cc_size: u16, // Size information, meaning depends of conversion type
}

#[derive(Debug, Clone)]
pub enum Conversion {
    Linear(Vec<f64>),
    TabularInterpolation(Vec<f64>),
    Tabular(Vec<f64>),
    Polynomial(Vec<f64>),
    Exponential(Vec<f64>),
    Logarithmic(Vec<f64>),
    Rational(Vec<f64>),
    Formula(String),
    TextTable(Vec<(f64, String)>),
    TextRangeTable(Vec<(f64, f64, String)>),
    Date,
    Time,
    Identity,
}

pub fn parse_cc3_block(
    rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
) -> i64 {
    rdr.seek_relative(target as i64 - position).unwrap(); // change buffer position
    let mut buf = vec![0u8; 46];
    rdr.read_exact(&mut buf).unwrap();
    position = target as i64 + 46;
    let mut block = Cursor::new(buf);
    let cc_block: Cc3Block = block.read_le().unwrap();
    let conversion: Conversion;
    match cc_block.cc_type {
        0 => {
            let mut buf = vec![0.0f64; 2];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Linear(buf);
            position += 16;
        }
        1 => {
            let mut buf = vec![0.0f64; cc_block.cc_size as usize * 2];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::TabularInterpolation(buf);
            position += cc_block.cc_size as i64 * 2 * 8;
        }
        2 => {
            let mut buf = vec![0.0f64; cc_block.cc_size as usize * 2];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Tabular(buf);
            position += cc_block.cc_size as i64 * 2 * 8;
        }
        6 => {
            let mut buf = vec![0.0f64; 6];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Polynomial(buf);
            position += 48;
        }
        7 => {
            let mut buf = vec![0.0f64; 7];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Exponential(buf);
            position += 56;
        }
        8 => {
            let mut buf = vec![0.0f64; 7];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Logarithmic(buf);
            position += 56;
        }
        9 => {
            let mut buf = vec![0.0f64; 6];
            rdr.read_f64_into::<LittleEndian>(&mut buf).unwrap();
            conversion = Conversion::Rational(buf);
            position += 48;
        }
        10 => {
            let mut buf = vec![0u8; 256];
            rdr.read_exact(&mut buf).unwrap();
            position += 256;
            let mut formula = String::new();
            ISO_8859_1
                .decode_to(&buf, DecoderTrap::Replace, &mut formula)
                .expect("formula text is latin1 encoded");
            formula = formula.trim_end_matches(char::from(0)).to_string();
            conversion = Conversion::Formula(formula);
        }
        11 => {
            let mut pairs: Vec<(f64, String)> = vec![(0.0f64, String::with_capacity(32)); cc_block.cc_size as usize];
            let mut val;
            let mut buf = vec![0u8; 32];
            let mut text = String::with_capacity(32);
            for index in 0..cc_block.cc_size as usize {
                val = rdr.read_f64::<LittleEndian>().unwrap();
                rdr.read_exact(&mut buf).unwrap();
                position += 40;
                ISO_8859_1
                    .decode_to(&buf, DecoderTrap::Replace, &mut text)
                    .expect("formula text is latin1 encoded");
                text = text.trim_end_matches(char::from(0)).to_string();
                pairs.insert(index, (val, text.clone()));
            }
            conversion = Conversion::TextTable(pairs);
        }
        12 => {
            let mut pairs_pointer: Vec<(f64, f64, u32)> = vec![(0.0f64, 0.0f64, 0u32); cc_block.cc_size as usize];
            let mut pairs_string: Vec<(f64, f64, String)> = vec![(0.0f64, 0.0f64, String::new()); cc_block.cc_size as usize];
            let mut low_range;
            let mut high_range;
            let mut text_pointer;
            for index in 0..cc_block.cc_size as usize {
                low_range = rdr.read_f64::<LittleEndian>().unwrap();
                high_range = rdr.read_f64::<LittleEndian>().unwrap();
                text_pointer = rdr.read_u32::<LittleEndian>().unwrap();
                position += 20;
                pairs_pointer.insert(index, (low_range, high_range, text_pointer));
            }
            for (index, (low_range, high_range, text_pointer)) in pairs_pointer.iter().enumerate() {
                let (_block_header, text, pos) = parse_tx(
                    rdr,
                    *text_pointer,
                    position,
                );
                position = pos;
                pairs_string.insert(index, (*low_range, *high_range, text));
            }
            conversion = Conversion::TextRangeTable(pairs_string);
        }
        _ => conversion = Conversion::Identity,
    }
    
    sharable.cc.insert(target, (cc_block, conversion));
    position
}

/// CE channel extension block struct, second sub block
#[derive(Debug, Clone)]
pub struct CeBlock {
    ce_id: [u8; 2],  // CE
    ce_len: u16,      // Length of block in bytes
    ce_extension_type: u16,   // extension type
    ce_extension: CeSupplement,
}

/// Either a DIM or CAN Supplemental block
#[derive(Debug, Clone)]
pub enum CeSupplement {
    DIM(DimBlock),
    CAN(CANBlock),
    None,
}

/// DIM Block supplement
#[derive(Debug, Clone)]
pub struct DimBlock {
    ce_module_number: u16, // Module number
    ce_address: u32,      // address
    ce_desc: String,   // description
    ce_ecu_id: String,   // ECU identifier
}

/// Vector CAN Block supplement
#[derive(Debug, Clone)]
pub struct CANBlock {
    ce_can_id: u32, // CAN identifier
    ce_can_index: u32,      // CAN channel index
    ce_message_name: String,   // message name
    ce_sender_name: String,   // sender name
}

/// parses Channel Extension block
fn parse_ce(rdr: &mut BufReader<&File>,
    target: u32,
    mut position: i64,
    sharable: &mut SharableBlocks3,
) -> i64 {
    rdr.seek_relative(target as i64 - position).unwrap(); // change buffer position
    let mut buf = vec![0u8; 6];
    rdr.read_exact(&mut buf).unwrap();
    position = target as i64 + 6;
    let mut block = Cursor::new(buf);
    let ce_id: [u8; 2] = block.read_le().unwrap();
    let ce_len: u16 = block.read_le().unwrap();
    let ce_extension_type: u16 = block.read_le().unwrap();
    
    let ce_extension: CeSupplement;
    if ce_extension_type == 0x02 {
        // Reads DIM
        let mut buf = vec![0u8; 118];
        rdr.read_exact(&mut buf).unwrap();
        position += 118;
        let mut block = Cursor::new(buf);
        let ce_module_number: u16 = block.read_le().unwrap();
        let ce_address: u32 = block.read_le().unwrap();
        let mut desc = vec![0u8; 80];
        let mut ce_desc = String::new();
        ISO_8859_1
            .decode_to(&desc, DecoderTrap::Replace, &mut ce_desc)
            .expect("DIM block description is latin1 encoded");
        ce_desc = ce_desc.trim_end_matches(char::from(0)).to_string();
        let mut desc = vec![0u8; 32];
        let mut ce_ecu_id = String::new();
        ISO_8859_1
            .decode_to(&desc, DecoderTrap::Replace, &mut ce_ecu_id)
            .expect("DIM block description is latin1 encoded");
        ce_ecu_id = ce_ecu_id.trim_end_matches(char::from(0)).to_string();
        ce_extension = CeSupplement::DIM(DimBlock {
            ce_module_number,
            ce_address,
            ce_desc,
            ce_ecu_id,
        });
    } else if ce_extension_type == 19 {
        // Reads CAN
        let mut buf = vec![0u8; 80];
        rdr.read_exact(&mut buf).unwrap();
        position += 80;
        let mut block = Cursor::new(buf);
        let ce_can_id: u32 = block.read_le().unwrap();
        let ce_can_index: u32 = block.read_le().unwrap();
        let mut message = vec![0u8; 36];
        let mut ce_message_name = String::new();
        ISO_8859_1
            .decode_to(&message, DecoderTrap::Replace, &mut ce_message_name)
            .expect("DIM block description is latin1 encoded");
        ce_message_name = ce_message_name.trim_end_matches(char::from(0)).to_string();
        let mut sender = vec![0u8; 32];
        let mut ce_sender_name = String::new();
        ISO_8859_1
            .decode_to(&sender, DecoderTrap::Replace, &mut ce_sender_name)
            .expect("DIM block description is latin1 encoded");
        ce_sender_name = ce_sender_name.trim_end_matches(char::from(0)).to_string();
        ce_extension = CeSupplement::CAN(CANBlock {
            ce_can_id,
            ce_can_index,
            ce_message_name,
            ce_sender_name,
        });
    } else {
        ce_extension = CeSupplement::None;
    }
    sharable.ce.insert(target, CeBlock {
        ce_id,
        ce_len,
        ce_extension_type,
        ce_extension,
    });
    position
}

/// parses mdfinfo structure to make channel names unique
/// creates channel names set and links master channels to set of channels
pub fn build_channel_db3(
    dg: &mut HashMap<u32, Dg3>,
    sharable: &SharableBlocks3,
    n_cg: u16,
    n_cn: u16,
) -> ChannelNamesSet3 {
    let mut channel_list: ChannelNamesSet3 = HashMap::with_capacity(n_cn as usize);
    let mut master_channel_list: HashMap<u32, String> = HashMap::with_capacity(n_cg as usize);
    // creating channel list for whole file and making channel names unique
    for (dg_position, dg) in dg.iter_mut() {
        for (record_id, cg) in dg.cg.iter_mut() {
            for (cn_position, cn) in cg.cn.iter_mut() {
                if channel_list.contains_key(&cn.unique_name) {
                    let mut changed: bool = false;
                    // create unique channel name
                    if let Some(ce) = sharable.ce.get(&cn.block1.cn_ce_source) {
                        match &ce.ce_extension {
                            CeSupplement::DIM(dim) => {
                                cn.unique_name.push_str(" ");
                                cn.unique_name.push_str(&dim.ce_ecu_id);
                                changed = true;
                            }
                            CeSupplement::CAN(can) => {
                                cn.unique_name.push_str(" ");
                                cn.unique_name.push_str(&can.ce_message_name);
                                changed = true;
                            }
                            _ => {}
                        }
                    }
                    // No souce name to make channel unique
                    if !changed {
                        // extend name with channel block position, unique
                        cn.unique_name.push_str(" ");
                        cn.unique_name.push_str(&cn_position.to_string());
                    }
                };
                let master = String::new();
                channel_list.insert(
                    cn.unique_name.clone(),
                    (
                        master, // computes at second step master channel name
                        *dg_position,
                        (cg.block_position, *record_id),
                        *cn_position,
                    ),
                );
                if cn.block1.cn_type != 0 {
                    // Master channel
                    master_channel_list.insert(cg.block_position, cn.unique_name.clone());
                }
            }
        }
    }
    // identifying master channels
    for (_dg_position, dg) in dg.iter_mut() {
        for (_record_id, cg) in dg.cg.iter_mut() {
            let mut cg_channel_list: HashSet<String> = HashSet::with_capacity(cg.block.cg_n_channels as usize);
            let mut master_channel_name: String = String::new();
            if let Some(name) = master_channel_list.get(&cg.block_position) {
                master_channel_name = name.to_string();
            } else {
                master_channel_name = format!("master_{}", cg.block_position); // default name in case no master is existing
            }
            for (_cn_record_position, cn) in cg.cn.iter_mut() {
                cg_channel_list.insert(cn.unique_name.clone());
                // assigns master in channel_list
                if let Some(id) = channel_list.get_mut(&cn.unique_name) {
                    id.0 = master_channel_name.clone();
                }
            }
            cg.channel_names = cg_channel_list;
            cg.master_channel_name = master_channel_name;
        }
    }
    channel_list
}