using System;
using System.Collections.Generic;
using SkylineBench.Bridge;

namespace SkylineBench.Tests
{
    public static class DirectionTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("direction: two-way is both", TwoWay));
            tests.Add(new KeyValuePair<string, Action>("direction: one-way follows lanes and invert", OneWay));
            tests.Add(new KeyValuePair<string, Action>("direction: laneless is both", Laneless));
        }

        static void TwoWay()
        {
            Assert.True(!Direction.IsOneWay(true, true), "fwd+bwd lanes is two-way");
            Assert.Equal(Direction.Both, Direction.Travel(true, true, false));
            Assert.Equal(Direction.Both, Direction.Travel(true, true, true));
        }

        static void OneWay()
        {
            Assert.True(Direction.IsOneWay(true, false), "fwd-only is one-way");
            Assert.Equal(Direction.StartToEnd, Direction.Travel(true, false, false));
            Assert.Equal(Direction.EndToStart, Direction.Travel(true, false, true));
            Assert.Equal(Direction.EndToStart, Direction.Travel(false, true, false));
            Assert.Equal(Direction.StartToEnd, Direction.Travel(false, true, true));
        }

        static void Laneless()
        {
            Assert.True(!Direction.IsOneWay(false, false), "no vehicle lanes is not one-way");
            Assert.Equal(Direction.Both, Direction.Travel(false, false, false));
        }
    }
}
