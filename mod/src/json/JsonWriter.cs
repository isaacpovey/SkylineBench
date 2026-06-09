using System;
using System.Globalization;
using System.Text;

namespace SkylineBench.Json
{
    /// <summary>Minimal fluent JSON serializer producing the contract's exact
    /// wire shapes. Commas/structure are managed automatically.</summary>
    public sealed class JsonWriter
    {
        private readonly StringBuilder _sb = new StringBuilder();
        private bool _needComma;

        private void Pre()
        {
            if (_needComma) _sb.Append(',');
            _needComma = false;
        }

        public JsonWriter BeginObject() { Pre(); _sb.Append('{'); return this; }
        public JsonWriter EndObject() { _sb.Append('}'); _needComma = true; return this; }
        public JsonWriter BeginArray() { Pre(); _sb.Append('['); return this; }
        public JsonWriter EndArray() { _sb.Append(']'); _needComma = true; return this; }

        public JsonWriter Name(string name)
        {
            Pre();
            WriteString(name);
            _sb.Append(':');
            return this;
        }

        public JsonWriter Value(string s)
        {
            Pre();
            if (s == null) _sb.Append("null"); else WriteString(s);
            _needComma = true;
            return this;
        }

        public JsonWriter Value(long n) { Pre(); _sb.Append(n.ToString(CultureInfo.InvariantCulture)); _needComma = true; return this; }

        public JsonWriter Value(double d)
        {
            Pre();
            _sb.Append(d.ToString("R", CultureInfo.InvariantCulture));
            _needComma = true;
            return this;
        }

        public JsonWriter Value(bool b) { Pre(); _sb.Append(b ? "true" : "false"); _needComma = true; return this; }
        public JsonWriter Null() { Pre(); _sb.Append("null"); _needComma = true; return this; }

        private void WriteString(string s)
        {
            _sb.Append('"');
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': _sb.Append("\\\""); break;
                    case '\\': _sb.Append("\\\\"); break;
                    case '\n': _sb.Append("\\n"); break;
                    case '\r': _sb.Append("\\r"); break;
                    case '\t': _sb.Append("\\t"); break;
                    default:
                        if (c < ' ') _sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        else _sb.Append(c);
                        break;
                }
            }
            _sb.Append('"');
        }

        public override string ToString() { return _sb.ToString(); }
    }
}
