using System;
using System.Collections.Generic;
using System.Threading;

namespace SkylineBench.Bridge
{
    public static class SimThread
    {
        private sealed class Job
        {
            public Action Work;
            public Exception Error;
            public readonly ManualResetEvent Done = new ManualResetEvent(false);
        }

        private static readonly Queue<Job> _queue = new Queue<Job>();
        private static readonly object _lock = new object();

        public static void Run(Action work, int timeoutMs)
        {
            var job = new Job { Work = work };
            lock (_lock) { _queue.Enqueue(job); }
            if (!job.Done.WaitOne(timeoutMs))
                throw new TimeoutException("sim-thread job timed out after " + timeoutMs + "ms");
            if (job.Error != null) throw job.Error;
        }

        public static T Run<T>(Func<T> work, int timeoutMs)
        {
            T result = default(T);
            Run(delegate { result = work(); }, timeoutMs);
            return result;
        }

        public static void DrainOnSimThread()
        {
            while (true)
            {
                Job job;
                lock (_lock) { if (_queue.Count == 0) return; job = _queue.Dequeue(); }
                try { job.Work(); }
                catch (Exception e) { job.Error = e; }
                finally { job.Done.Set(); }
            }
        }
    }
}
