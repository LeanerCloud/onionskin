// The Acrobat forms API subset: `event`, `this.getField`, `app.alert`,
// `util.printf`, `util.printd`, `util.scand`, and the AF functions the
// Format, Keystroke, Validate and Calculate panels write. Everything a
// script can reach is defined here; there is no I/O to reach.
//
// The host sets `__input` before this runs: { kind, target, value, change,
// willCommit, fields: { name: value } }. It reads `__output()` after the
// field's own script ran.

var __alerts = [];
var __changed = {};

function __isNumeric(text) {
  return typeof text === "string" && text.trim() !== "" && !isNaN(Number(text));
}

function __Field(name) {
  this.name = name;
}
Object.defineProperty(__Field.prototype, "value", {
  get: function () {
    var raw = __input.fields[this.name];
    if (raw === undefined || raw === null) return "";
    return __isNumeric(raw) ? Number(raw) : raw;
  },
  set: function (value) {
    var text = value === null || value === undefined ? "" : String(value);
    __input.fields[this.name] = text;
    __changed[this.name] = text;
  },
});
Object.defineProperty(__Field.prototype, "valueAsString", {
  get: function () {
    var raw = __input.fields[this.name];
    return raw === undefined || raw === null ? "" : String(raw);
  },
});

var event = {
  name: __input.kind,
  type: "Field",
  value: __input.value,
  change: __input.change,
  willCommit: __input.willCommit,
  rc: true,
  target: new __Field(__input.target),
  targetName: __input.target,
};

var __doc = {
  getField: function (name) {
    if (!(name in __input.fields)) return null;
    return new __Field(name);
  },
  numFields: Object.keys(__input.fields).length,
};

var app = {
  alert: function (message) {
    __alerts.push(typeof message === "object" && message !== null ? String(message.cMsg) : String(message));
    return 1;
  },
  beep: function () {},
  viewerType: "Reader",
  viewerVersion: 21,
  platform: "UNIX",
};

// --- numbers ---------------------------------------------------------------

function AFMakeNumber(value) {
  if (typeof value === "number") return value;
  if (value === null || value === undefined) return null;
  var text = String(value).trim().replace(/[^0-9,.\-+eE]/g, "");
  if (text === "") return null;
  // A comma as the only separator is a decimal comma.
  if (text.indexOf(",") >= 0 && text.indexOf(".") < 0 && /,\d{1,2}$/.test(text)) {
    text = text.replace(/\./g, "").replace(",", ".");
  } else {
    text = text.replace(/,/g, "");
  }
  var number = Number(text);
  return isNaN(number) ? null : number;
}

function __group(digits, separator) {
  var out = "";
  for (var i = 0; i < digits.length; i++) {
    if (i > 0 && (digits.length - i) % 3 === 0) out += separator;
    out += digits.charAt(i);
  }
  return out;
}

// sepStyle: 0 1,234.56  1 1234.56  2 1.234,56  3 1234,56  4 1'234.56
function __formatNumber(number, decimals, sepStyle) {
  var fixed = Math.abs(number).toFixed(decimals);
  var parts = fixed.split(".");
  var groups = ["," , "", ".", "", "'"][sepStyle] || "";
  var point = sepStyle === 2 || sepStyle === 3 ? "," : ".";
  var whole = groups === "" ? parts[0] : __group(parts[0], groups);
  return parts.length > 1 ? whole + point + parts[1] : whole;
}

function AFNumber_Format(nDec, sepStyle, negStyle, currStyle, strCurrency, bCurrencyPrepend) {
  var number = AFMakeNumber(event.value);
  if (number === null) {
    event.value = "";
    return;
  }
  var text = __formatNumber(number, nDec, sepStyle);
  if (strCurrency) text = bCurrencyPrepend ? strCurrency + text : text + strCurrency;
  if (number < 0) {
    text = negStyle === 2 || negStyle === 3 ? "(" + text + ")" : "-" + text;
  }
  event.value = text;
}

function __numberKeystroke(label) {
  if (event.willCommit) {
    var value = String(event.value).trim();
    if (value !== "" && (!/^[-+]?[0-9.,' ]+$/.test(value) || AFMakeNumber(value) === null)) {
      app.alert("The value entered does not match the format of the field [ " + event.targetName + " ]");
      event.rc = false;
    }
    return;
  }
  if (!/^[0-9.,\-+ ]*$/.test(event.change)) event.rc = false;
}

function AFNumber_Keystroke() {
  __numberKeystroke();
}

function AFPercent_Format(nDec, sepStyle, bPercentPrepend) {
  var number = AFMakeNumber(event.value);
  if (number === null) {
    event.value = "";
    return;
  }
  var text = __formatNumber(number * 100, nDec, sepStyle);
  if (number < 0) text = "-" + text;
  event.value = bPercentPrepend ? "%" + text : text + "%";
}

function AFPercent_Keystroke() {
  __numberKeystroke();
}

// --- dates -----------------------------------------------------------------

var __months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
var __days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

function __pad(number, width) {
  var text = String(number);
  while (text.length < width) text = "0" + text;
  return text;
}

var util = {
  printd: function (format, date) {
    if (typeof format === "number") format = ["mm/dd/yyyy", "yy/mm/dd HH:MM:ss", "yyyymmddHHMMss"][format] || "mm/dd/yyyy";
    var tokens = /yyyy|yy|mmmm|mmm|mm|m|dddd|ddd|dd|d|HH|H|hh|h|MM|M|ss|s|tt|t|\\./g;
    return format.replace(tokens, function (token) {
      var hours = date.getHours();
      switch (token) {
        case "yyyy": return __pad(date.getFullYear(), 4);
        case "yy": return __pad(date.getFullYear() % 100, 2);
        case "mmmm": return __months[date.getMonth()];
        case "mmm": return __months[date.getMonth()].substring(0, 3);
        case "mm": return __pad(date.getMonth() + 1, 2);
        case "m": return String(date.getMonth() + 1);
        case "dddd": return __days[date.getDay()];
        case "ddd": return __days[date.getDay()].substring(0, 3);
        case "dd": return __pad(date.getDate(), 2);
        case "d": return String(date.getDate());
        case "HH": return __pad(hours, 2);
        case "H": return String(hours);
        case "hh": return __pad(hours % 12 === 0 ? 12 : hours % 12, 2);
        case "h": return String(hours % 12 === 0 ? 12 : hours % 12);
        case "MM": return __pad(date.getMinutes(), 2);
        case "M": return String(date.getMinutes());
        case "ss": return __pad(date.getSeconds(), 2);
        case "s": return String(date.getSeconds());
        case "tt": return hours < 12 ? "am" : "pm";
        case "t": return hours < 12 ? "a" : "p";
        default: return token.substring(1);
      }
    });
  },
  scand: function (format, text) {
    return __parseDate(String(text), format);
  },
  printf: function (format) {
    var args = Array.prototype.slice.call(arguments, 1);
    var at = 0;
    return String(format).replace(/%([,0-9]*)\.?(\d*)([dfsx%])/g, function (all, flags, precision, kind) {
      if (kind === "%") return "%";
      var arg = args[at++];
      if (kind === "d") return String(Math.trunc(Number(arg)));
      if (kind === "x") return Math.trunc(Number(arg)).toString(16);
      if (kind === "f") {
        var digits = precision === "" ? 6 : Number(precision);
        var sep = flags.indexOf(",") === 0 ? Number(flags.charAt(1) || "0") : 1;
        var number = Number(arg);
        var text = __formatNumber(number, digits, sep);
        return number < 0 ? "-" + text : text;
      }
      return String(arg);
    });
  },
};

// Reads a date the way the user typed it, in the field's order of day,
// month and year: numbers, or month names.
function __parseDate(text, format) {
  var numbers = text.match(/\d+/g) || [];
  var names = text.match(/[A-Za-z]+/g) || [];
  var month = -1;
  for (var i = 0; i < names.length && month < 0; i++) {
    for (var m = 0; m < 12; m++) {
      if (__months[m].toLowerCase().indexOf(names[i].toLowerCase().substring(0, 3)) === 0) month = m;
    }
  }
  var order = [];
  var fields = format.match(/y+|m+|d+|H+|h+|M+|s+/g) || [];
  for (var f = 0; f < fields.length; f++) order.push(fields[f].charAt(0));
  var year = -1, day = -1, hours = 0, minutes = 0, seconds = 0;
  var next = 0;
  for (var o = 0; o < order.length && next < numbers.length; o++) {
    var kind = order[o];
    if (kind === "m" && month >= 0) continue;
    var value = Number(numbers[next++]);
    if (kind === "y") year = value < 100 ? (value < 50 ? 2000 + value : 1900 + value) : value;
    else if (kind === "m") month = value - 1;
    else if (kind === "d") day = value;
    else if (kind === "H" || kind === "h") hours = value;
    else if (kind === "M") minutes = value;
    else if (kind === "s") seconds = value;
  }
  if (year < 0 && order.indexOf("y") < 0) year = new Date().getFullYear();
  if (day < 0 && order.indexOf("d") < 0) day = 1;
  if (year < 0 || month < 0 || month > 11 || day < 1) return null;
  var date = new Date(year, month, day, hours, minutes, seconds);
  if (date.getMonth() !== month || date.getDate() !== day) return null;
  return date;
}

var __dateFormats = ["m/d", "m/d/yy", "mm/dd/yy", "mm/yy", "d-mmm", "d-mmm-yy", "dd-mmm-yy", "yy-mm-dd", "mmm-yy", "mmmm-yy", "mmm d, yyyy", "mmmm d, yyyy", "m/d/yy h:MM tt", "m/d/yy HH:MM"];

function AFDate_FormatEx(format) {
  if (String(event.value).trim() === "") return;
  var date = __parseDate(String(event.value), format);
  if (date) event.value = util.printd(format, date);
}

function AFDate_Format(index) {
  AFDate_FormatEx(__dateFormats[index] || "m/d/yy");
}

function AFDate_KeystrokeEx(format) {
  if (!event.willCommit || String(event.value).trim() === "") return;
  if (!__parseDate(String(event.value), format)) {
    app.alert("Invalid date/time: please ensure that the date/time exists. Field [ " + event.targetName + " ] should match format " + format);
    event.rc = false;
  }
}

function AFDate_Keystroke(index) {
  AFDate_KeystrokeEx(__dateFormats[index] || "m/d/yy");
}

var __timeFormats = ["HH:MM", "h:MM tt", "HH:MM:ss", "h:MM:ss tt"];

function AFTime_FormatEx(format) {
  if (String(event.value).trim() === "") return;
  var parts = (String(event.value).match(/\d+/g) || []).map(Number);
  if (parts.length < 2) return;
  var hours = parts[0];
  if (/p/i.test(String(event.value)) && hours < 12) hours += 12;
  var date = new Date(2000, 0, 1, hours, parts[1], parts[2] || 0);
  event.value = util.printd(format, date);
}

function AFTime_Format(index) {
  AFTime_FormatEx(__timeFormats[index] || "HH:MM");
}

function AFTime_Keystroke() {}

// --- special ---------------------------------------------------------------

var __specialMasks = ["99999", "99999-9999", "(999) 999-9999", "999-99-9999"];

function AFSpecial_Format(psf) {
  var digits = String(event.value).replace(/\D/g, "");
  if (digits === "") return;
  var mask = __specialMasks[psf];
  if (psf === 1 && digits.length === 5) mask = "99999";
  var out = "";
  var at = 0;
  for (var i = 0; i < mask.length && at < digits.length; i++) {
    out += mask.charAt(i) === "9" ? digits.charAt(at++) : mask.charAt(i);
  }
  event.value = out;
}

function AFSpecial_Keystroke(psf) {
  if (!/^[0-9 ()\-]*$/.test(event.change)) {
    event.rc = false;
    return;
  }
  if (event.willCommit) {
    var digits = String(event.value).replace(/\D/g, "").length;
    var wanted = [[5], [5, 9], [10], [9]][psf] || [];
    if (digits > 0 && wanted.indexOf(digits) < 0) {
      app.alert("The value entered does not match the format of the field [ " + event.targetName + " ]");
      event.rc = false;
    }
  }
}

function AFSpecial_KeystrokeEx(mask) {
  if (event.willCommit && String(event.value).length > 0 && String(event.value).length !== String(mask).length) {
    app.alert("The value entered does not match the format of the field [ " + event.targetName + " ]");
    event.rc = false;
  }
}

// --- validate and calculate --------------------------------------------------

function AFRange_Validate(bGreaterThan, nGreaterThan, bLessThan, nLessThan) {
  if (String(event.value).trim() === "") return;
  var number = AFMakeNumber(event.value);
  if (number === null) return;
  var message = "";
  if (bGreaterThan && bLessThan && (number < nGreaterThan || number > nLessThan)) {
    message = "The value entered must be greater than or equal to " + nGreaterThan + " and less than or equal to " + nLessThan + ".";
  } else if (bGreaterThan && !bLessThan && number < nGreaterThan) {
    message = "The value entered must be greater than or equal to " + nGreaterThan + ".";
  } else if (bLessThan && !bGreaterThan && number > nLessThan) {
    message = "The value entered must be less than or equal to " + nLessThan + ".";
  }
  if (message) {
    app.alert(message);
    event.rc = false;
  }
}

function AFSimple_Calculate(cFunction, cFields) {
  var names = typeof cFields === "string" ? cFields.split(",") : cFields;
  var values = [];
  for (var i = 0; i < names.length; i++) {
    var name = String(names[i]).trim();
    var prefix = name + ".";
    for (var key in __input.fields) {
      if (key === name || key.indexOf(prefix) === 0) {
        var number = AFMakeNumber(__input.fields[key]);
        values.push(number === null ? 0 : number);
      }
    }
  }
  var result = 0;
  switch (String(cFunction).toUpperCase()) {
    case "SUM":
      for (var s = 0; s < values.length; s++) result += values[s];
      break;
    case "PRD":
      result = values.length ? 1 : 0;
      for (var p = 0; p < values.length; p++) result *= values[p];
      break;
    case "AVG":
      for (var a = 0; a < values.length; a++) result += values[a];
      result = values.length ? result / values.length : 0;
      break;
    case "MIN":
      result = values.length ? Math.min.apply(null, values) : 0;
      break;
    case "MAX":
      result = values.length ? Math.max.apply(null, values) : 0;
      break;
  }
  event.value = result;
}

function __output() {
  return JSON.stringify({
    value: event.value === null || event.value === undefined ? "" : String(event.value),
    rc: event.rc !== false,
    alerts: __alerts,
    changed: __changed,
  });
}
