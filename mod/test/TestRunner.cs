using System;
using System.Collections.Generic;

namespace SkylineBench.Tests
{
    public static class Assert
    {
        public static void Equal(string expected, string actual)
        {
            if (!string.Equals(expected, actual, StringComparison.Ordinal))
                throw new Exception("Expected:\n  " + expected + "\nActual:\n  " + actual);
        }

        public static void Equal(double expected, double actual)
        {
            if (Math.Abs(expected - actual) > 1e-9)
                throw new Exception("Expected " + expected + " but got " + actual);
        }

        public static void True(bool cond, string msg)
        {
            if (!cond) throw new Exception("Expected true: " + msg);
        }
    }

    public static class TestRunner
    {
        public static int Main()
        {
            var tests = new List<KeyValuePair<string, Action>>();
            JsonWriterTests.Register(tests);
            JsonReaderTests.Register(tests);
            HttpQueryTests.Register(tests);

            int passed = 0, failed = 0;
            foreach (var t in tests)
            {
                try { t.Value(); Console.WriteLine("ok   - " + t.Key); passed++; }
                catch (Exception e) { Console.WriteLine("FAIL - " + t.Key + "\n      " + e.Message.Replace("\n", "\n      ")); failed++; }
            }
            Console.WriteLine(string.Format("\n{0} passed, {1} failed", passed, failed));
            return failed == 0 ? 0 : 1;
        }
    }
}
