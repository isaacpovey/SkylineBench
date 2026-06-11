using System;
using System.IO;
using System.Net;
using System.Text;
using System.Threading;

namespace SkylineBench.Http
{
    public delegate HttpReply Dispatch(string method, string path, HttpQuery query, string body);

    public struct HttpReply
    {
        public int Status;
        public string ContentType;
        public string Body;
        public byte[] Bytes;
        public static HttpReply Json(int status, string body) { return new HttpReply { Status = status, ContentType = "application/json", Body = body }; }
        public static HttpReply Text(int status, string body) { return new HttpReply { Status = status, ContentType = "text/plain", Body = body }; }
        public static HttpReply Png(byte[] bytes) { return new HttpReply { Status = 200, ContentType = "image/png", Bytes = bytes }; }
    }

    public sealed class HttpServer
    {
        private readonly HttpListener _listener = new HttpListener();
        private readonly Dispatch _dispatch;
        private Thread _thread;
        private volatile bool _running;

        public HttpServer(int port, Dispatch dispatch)
        {
            _dispatch = dispatch;
            _listener.Prefixes.Add("http://127.0.0.1:" + port + "/");
        }

        public void Start()
        {
            _listener.Start();
            _running = true;
            _thread = new Thread(Loop) { IsBackground = true, Name = "SkylineBenchHttp" };
            _thread.Start();
            Log.Info("HTTP server listening on " + GetPrefix());
        }

        private string GetPrefix() { foreach (var p in _listener.Prefixes) return p; return ""; }

        public void Stop()
        {
            _running = false;
            try { _listener.Stop(); } catch { }
            try { _listener.Close(); } catch { }
        }

        private void Loop()
        {
            while (_running)
            {
                HttpListenerContext ctx;
                try { ctx = _listener.GetContext(); }
                catch { if (!_running) return; continue; }
                try { Handle(ctx); }
                catch (Exception e) { Log.Error("request handling failed: " + e); }
            }
        }

        private void Handle(HttpListenerContext ctx)
        {
            var req = ctx.Request;
            string body = "";
            if (req.HasEntityBody)
                using (var sr = new StreamReader(req.InputStream, req.ContentEncoding ?? Encoding.UTF8))
                    body = sr.ReadToEnd();

            string path = req.Url.AbsolutePath;
            var query = HttpQuery.Parse(req.Url.Query);

            HttpReply reply;
            try { reply = _dispatch(req.HttpMethod, path, query, body); }
            catch (Exception e) { reply = HttpReply.Text(500, "internal: " + e.Message); }

            byte[] buf = reply.Bytes != null ? reply.Bytes : Encoding.UTF8.GetBytes(reply.Body ?? "");
            ctx.Response.StatusCode = reply.Status;
            ctx.Response.ContentType = reply.ContentType ?? "text/plain";
            ctx.Response.ContentLength64 = buf.Length;
            ctx.Response.OutputStream.Write(buf, 0, buf.Length);
            ctx.Response.OutputStream.Close();
        }
    }
}
