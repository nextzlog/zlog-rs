use zlog_cfg::script::{Judged, Limits, ScriptQso, Sums, Vm};
use zlog_core::{Band, Mode};

const PIN: &str = r#"
local keys = {"call", "sent", "rcvd", "name", "note", "band", "mode", "srst", "rrst", "tx", "time", "dupe", "counted"}

function init()
	return {seen = 0}
end

function add(state, qso)
	local parts = {}
	for _, k in keys do
		table.insert(parts, k .. "=" .. tostring(qso[k]) .. ":" .. type(qso[k]))
	end
	local n = 0
	for _ in qso do
		n += 1
	end
	table.insert(parts, "fields=" .. n)
	state.seen += 1
	return state, {score = state.seen, mul1 = table.concat(parts, " "), mul2 = qso.band, invalid = nil}
end

function total(state, sums)
	local bands = table.concat(zlog.bands, ",")
	local modes = table.concat(zlog.modes, ",")
	assert(zlog.api == 1)
	assert(bands == "1.9,3.5,7,10,14,18,21,24,28,50,144,430,1200,2400,5600,10.1G,10.4G,24G,47G,77G,135G,248G,Target,Unknown", bands)
	assert(modes == "CW,SSB,FM,AM,RTTY,FT4,FT8,Other,DV", modes)
	return sums.score * 1000000 + sums.mults * 1000 + sums.qsos + state.seen * 100000000
end
"#;

#[test]
fn the_api_of_version_1_is_pinned() {
    let mut v = zlog_lua::LuaVm::compile("pin", PIN, &Limits::default()).unwrap();
    v.reset().unwrap();
    let q = ScriptQso {
        call: "JA1ZLO/1".into(),
        band: Band::B430,
        mode: Mode::Fm,
        rcvd: "10HS".into(),
        sent: "1001M".into(),
        name: "OP1".into(),
        note: "ノート".into(),
        srst: 59,
        rrst: 57,
        tx: 2,
        time: (2026, 8, 1, 9, 5, 7),
        dupe: true,
        counted: false,
    };
    assert_eq!(
        v.add(&q).unwrap(),
        Judged {
            points: 1,
            multis: vec![
                "call=JA1ZLO/1:string sent=1001M:string rcvd=10HS:string name=OP1:string note=ノート:string band=430:string mode=FM:string srst=59:number rrst=57:number tx=2:number time=2026-08-01T09:05:07:string dupe=true:boolean counted=false:boolean fields=13".into(),
                "430".into(),
            ],
            invalid: None,
        }
    );
    let sums = Sums {
        qsos: 3,
        points: 4,
        multis: 5,
    };
    assert_eq!(v.total(sums).unwrap(), 104_005_003);
}

#[test]
fn verdict_defaults_and_unknown_fields() {
    let src = "function init() return {} end\nfunction add(s, q) return s, {future = 1} end\nfunction total(s, t) return 0 end";
    let mut v = zlog_lua::LuaVm::compile("d", src, &Limits::default()).unwrap();
    v.reset().unwrap();
    let q = ScriptQso {
        call: "X".into(),
        band: Band::B7,
        mode: Mode::Cw,
        rcvd: String::new(),
        sent: String::new(),
        name: String::new(),
        note: String::new(),
        srst: 0,
        rrst: 0,
        tx: 0,
        time: (2026, 1, 1, 0, 0, 0),
        dupe: false,
        counted: true,
    };
    assert_eq!(
        v.add(&q).unwrap(),
        Judged {
            points: 0,
            multis: vec![String::new(), String::new()],
            invalid: None,
        }
    );
}
