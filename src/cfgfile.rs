use std::collections::{HashMap, HashSet};

use derive_more::Display;
use unicode_xid::UnicodeXID;
use nom::{
	IResult,
	branch::alt,
	bytes::complete::{tag, take_until},
	character::complete::{anychar, i32, multispace1, newline, not_line_ending},
	combinator::{all_consuming, map, opt, value, verify},
	error::{VerboseError, context, convert_error},
	multi::many0,
	sequence::{delimited, pair, preceded, terminated, tuple},
};

use crate::{PaaError, PaaError::*, PaaResult, PaaType, ArgbSwizzle, TextureEncodingSettings, TextureErrorMetrics, TextureMipmapFilter};


fn parse_single_line_comment(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), pair(tag("//"), context("single line comment", tuple((not_line_ending, opt(newline))))))(i)
}


fn parse_multi_line_comment(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), tuple((tag("/*"), context("multi-line comment", take_until("*/")), tag("*/"))))(i)
}


#[test]
fn comments() {
	assert_eq!(parse_single_line_comment("// Good"), Ok(("", ())));
	assert_eq!(parse_single_line_comment("// comment\nnot a comment\n"), Ok(("not a comment\n", ())));
	assert_eq!(parse_multi_line_comment("/* Good /* \n //*/not a comment"), Ok(("not a comment", ())));
	assert!(parse_single_line_comment("/ Bad").is_err());
	assert!(parse_multi_line_comment("/* Bad").is_err());
	assert!(parse_multi_line_comment("Bad */").is_err());
}


fn construct_ident(args: (char, Vec<char>)) -> ConfigIdent {
	let (start, cont) = args;
	let mut inner = String::new();
	inner.push(start);
	inner.extend(cont);
	ConfigIdent::from(&inner)
}


fn parse_ident(i: &str) -> IResult<&str, ConfigIdent, VerboseError<&str>> {
	map(
		pair(
			verify(anychar, |c: &char| UnicodeXID::is_xid_start(*c)),
			many0(verify(anychar, |c: &char| UnicodeXID::is_xid_continue(*c)))),
	construct_ident)(i)
}


fn parse_value(i: &str) -> IResult<&str, ConfigValue, VerboseError<&str>> {
	alt((
		map(i32, ConfigValue::Integer),
		map(delimited(tag("\""), take_until("\""), tag("\"")), |s: &str| ConfigValue::String(String::from(s))),
		map(parse_ident, ConfigValue::Ident),
	))(i)
}


fn parse_property(i: &str) -> IResult<&str, ConfigProperty, VerboseError<&str>> {
	tuple((
			parse_ident,
			context("equals sign", with_ws_or_comments(tag("="))),
			context("property value", with_ws_or_comments(parse_value)),))
		(i)
		.map(|args: (&str, (ConfigIdent, &str, ConfigValue))| {
			let (left, (ident, _, value)) = args;
			(left, ConfigProperty { ident, value })
		})
}


fn parse_class(i: &str) -> IResult<&str, ConfigClass, VerboseError<&str>> {
	let class_name = context("class name", with_ws_or_comments(parse_ident));
	let parent_class_name = context("parent class name", opt(preceded(with_ws_or_comments(tag(":")), with_ws_or_comments(parse_ident))));
	let children = context("children", terminated_list(parse_item, ";"));

	#[allow(clippy::type_complexity)]
	tuple((
		context("class tag", tag("class")),
		class_name,
		parent_class_name,
		context("opening brace", with_ws_or_comments(tag("{"))),
		children,
		context("closing brace", tag("}")),))
	(i)
	.map(|args: (&str, (&str, ConfigIdent, Option<ConfigIdent>, &str, Vec<ConfigItem>, &str))| {
		let (left, (_, classname, parent_class, _, children, _)) = args;
		let inherit_classname = parent_class;
		(left, ConfigClass { classname, inherit_classname, children})
	})
}


fn parse_item(i: &str) -> IResult<&str, ConfigItem, VerboseError<&str>> {
	alt((
		map(parse_property, ConfigItem::Property),
		map(parse_class, ConfigItem::Class)
	))(i)
}


#[test]
fn property() {
	assert_eq!(parse_ident("dynRange").unwrap(), ("", ConfigIdent::from("dynRange")));
	assert_eq!(parse_value("\"Hello\"").unwrap(), ("", ConfigValue::String(String::from("Hello"))));
	assert_eq!(parse_value("-20").unwrap(), ("", ConfigValue::Integer(-20)));
	assert_eq!(parse_property("dynRange = /* comment */1").unwrap(), ("", (ConfigProperty { ident: ConfigIdent::from("dynRange"), value: ConfigValue::Integer(1)})));
}


fn wscom0(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), many0(alt((parse_single_line_comment, parse_multi_line_comment, value((), multispace1)))))(i)
}


fn with_ws_or_comments<'a, F: 'a, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O, VerboseError<&'a str>>
where
	F: FnMut(&'a str) -> IResult<&'a str, O, VerboseError<&'a str>>,
{
	delimited(wscom0, inner, wscom0)
}


#[test]
fn with_whitespace() {
	assert_eq!(with_ws_or_comments(parse_ident)(" /* comment */ ident // another comment").unwrap(), ("", ConfigIdent::from("ident")));
}


fn terminated_list<'a, F: 'a, O>(inner: F, delimiter: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<O>, VerboseError<&'a str>>
where
	F: FnMut(&'a str) -> IResult<&'a str, O, VerboseError<&'a str>>,
{
	many0(terminated(with_ws_or_comments(inner), with_ws_or_comments(tag(delimiter))))
}


#[derive(Debug, Display, PartialEq, Eq, Hash, Clone)]
struct ConfigIdent {
	inner: String,
}


impl PartialEq<&str> for ConfigIdent {
	fn eq(&self, other: &&str) -> bool {
		self.inner.to_uppercase() == other.to_uppercase()
	}
}


impl ConfigIdent {
	fn from(inner: &str) -> Self {
		let inner = String::from(inner);
		Self { inner }
	}

	fn normalized(self) -> Self {
		Self { inner: self.inner.to_uppercase() }
	}
}


#[derive(Debug, PartialEq, Eq, Clone)]
enum ConfigItem {
	Property(ConfigProperty),
	Class(ConfigClass),
}


impl ConfigItem {
	fn normalized(self) -> Self {
		match self {
			ConfigItem::Property(p) => ConfigItem::Property(p.normalized()),
			ConfigItem::Class(c) => ConfigItem::Class(c.normalized()),
		}
	}


	fn get_ident(&self) -> &ConfigIdent {
		match self {
			ConfigItem::Property(p) => &p.ident,
			ConfigItem::Class(c) => &c.classname,
		}
	}
}


#[derive(Debug, PartialEq, Eq, Clone)]
struct ConfigClass {
	classname: ConfigIdent,
	inherit_classname: Option<ConfigIdent>,
	children: Vec<ConfigItem>,
}


impl ConfigClass {
	fn normalized(self) -> Self {
		let classname = self.classname.normalized();
		let inherit_classname = self.inherit_classname.map(ConfigIdent::normalized);
		let mut children_set: HashSet<ConfigIdent> = HashSet::new();
		let mut children = vec![];

		for c in self.children {
			let c = c.normalized();
			if children_set.contains(c.get_ident()) { continue; };
			let _ = children_set.insert(c.get_ident().clone());
			children.push(c);
		};

		ConfigClass { classname, inherit_classname, children }
	}


	fn into_settings(self, siblings: &HashMap<String, TextureEncodingSettings>) -> PaaResult<(String, TextureEncodingSettings)> {
		let prop = |ident| self.children.iter()
			.find(|i| matches!(i, ConfigItem::Property(_)) && *i.get_ident() == ident)
			.map(|p| match p { ConfigItem::Property(p) => p.clone(), _ => unreachable!() } );

		let mut settings = TextureEncodingSettings::default();

		if let Some(parent) = self.inherit_classname {
			settings = *siblings.get(&parent.inner).ok_or(TexconvertInvalidInherit(parent.inner))?;
		};

		let suffix = prop("name")
			.and_then(ConfigProperty::try_into_string)
			.and_then(ConfigClass::name_to_suffix).ok_or(TexconvertNoName)?;

		let enable_dxt = prop("enableDXT")
			.and_then(ConfigProperty::try_into_bool)
			.map(|e| if e { String::from("DXT5") } else { String::from("ARGB8888") });

		let format = prop("format")
			.and_then(ConfigProperty::try_into_string)
			.or(enable_dxt)
			.and_then(|s| s.parse::<PaaType>().ok());

		let dynrange = prop("dynrange")
			.and_then(ConfigProperty::try_into_bool);

		let autoreduce = prop("autoreduce")
			.and_then(ConfigProperty::try_into_bool);

		let mipmap_filter = prop("mipmapFilter")
			.and_then(ConfigProperty::try_into_ident)
			.and_then(|i| i.inner.parse::<TextureMipmapFilter>().ok());

		let error_metrics = prop("errorMetrics")
			.and_then(ConfigProperty::try_into_ident)
			.and_then(|i| i.inner.parse::<TextureErrorMetrics>().ok());

		let swizzle = {
			let swiz_a = prop("channelSwizzleA").and_then(|p| p.try_into_string())
				.unwrap_or_else(|| "A".into());
			let swiz_r = prop("channelSwizzleR").and_then(|p| p.try_into_string())
				.unwrap_or_else(|| "R".into());
			let swiz_g = prop("channelSwizzleG").and_then(|p| p.try_into_string())
				.unwrap_or_else(|| "G".into());
			let swiz_b = prop("channelSwizzleB").and_then(|p| p.try_into_string())
				.unwrap_or_else(|| "B".into());
			ArgbSwizzle::parse_argb(&swiz_a, &swiz_r, &swiz_g, &swiz_b)?
		};

		if let Some(format) = format {
			settings = TextureEncodingSettings { format, ..settings };
		};

		if let Some(dynrange) = dynrange {
			settings = TextureEncodingSettings { dynrange: Some(dynrange), ..settings };
		};

		if let Some(autoreduce) = autoreduce {
			settings = TextureEncodingSettings { autoreduce, ..settings };
		};

		if let Some(mipmap_filter) = mipmap_filter {
			settings = TextureEncodingSettings { mipmap_filter: Some(mipmap_filter), ..settings };
		};

		if let Some(error_metrics) = error_metrics {
			settings = TextureEncodingSettings { error_metrics: Some(error_metrics), ..settings };
		};

		settings = TextureEncodingSettings { swizzle, ..settings };

		Ok((suffix, settings))
	}


	fn name_to_suffix(name: String) -> Option<String> {
		if name.starts_with("*_") && name.ends_with(".*") {
			Some(String::from(&name[2..name.len()-2]).to_uppercase())
		}
		else {
			None
		}
	}
}


#[derive(Debug, Display, PartialEq, Eq, Clone)]
#[display(fmt = "{} = {};", ident, value)]
struct ConfigProperty {
	ident: ConfigIdent,
	value: ConfigValue,
}


impl ConfigProperty {
	fn normalized(self) -> Self {
		let ident = self.ident.normalized();
		let value = self.value.normalized();
		Self { ident, value }
	}


	fn try_into_string(self) -> Option<String> {
		match self.value {
			ConfigValue::String(ref s) => Some(s.clone()),
			_ => None,
		}
	}


	fn try_into_ident(self) -> Option<ConfigIdent> {
		match self.value {
			ConfigValue::Ident(ref i) => Some(i.clone()),
			_ => None,
		}
	}


	fn try_into_bool(self) -> Option<bool> {
		match self.value {
			ConfigValue::Integer(i) => Some(i != 0),
			_ => None,
		}
	}
}


#[derive(Debug, Display, PartialEq, Eq, Clone)]
enum ConfigValue {
	#[display(fmt = "{}", _0)]
	Integer(i32),
	#[display(fmt = "\"{}\"", _0)]
	String(String),
	#[display(fmt = "{}", _0)]
	Ident(ConfigIdent),
}


impl std::str::FromStr for ConfigValue {
	type Err = PaaError;

	fn from_str(input: &str) -> PaaResult<Self> {
		let (_, result) = parse_value(input)
			.map_err(|e| TexconvertParseError(e.map(|e| convert_error(input, e))))?;
		Ok(result)
	}
}


impl ConfigValue {
	fn normalized(self) -> Self {
		match self {
			ConfigValue::Ident(i) => ConfigValue::Ident(i.normalized()),
			s => s,
		}
	}
}


pub(crate) fn try_parse_texconvert(input: &str) -> PaaResult<HashMap<String, TextureEncodingSettings>> {
	let (_, items) = all_consuming(terminated_list(parse_item, ";"))(input)
		.map_err(|e| TexconvertParseError(e.map(|e| {eprintln!("{:?}", e); convert_error(input, e)})))?;

	let mut hints: Option<ConfigClass> = None;
	let mut result: HashMap<String, TextureEncodingSettings> = HashMap::new();

	for i in items {
		if let ConfigItem::Class(c) = i {
			if c.classname == "TextureHints" {
				hints = Some(c);
			};
		};
	};

	let hints = if let Some(hints) = hints { hints } else { return Ok(HashMap::new()); };

	let mut classname_map: HashMap<String, TextureEncodingSettings> = HashMap::new();

	let child_classes = hints.children.into_iter()
		.filter_map(|c| if let ConfigItem::Class(c) = c { Some(c.normalized()) } else { None });

	for c in child_classes {
		let classname = c.classname.clone().normalized().to_string();
		let (suffix, settings) = c.into_settings(&classname_map)?;
		let _ = classname_map.insert(classname, settings);
		let _ = result.insert(suffix, settings);
	};

	Ok(result)
}
