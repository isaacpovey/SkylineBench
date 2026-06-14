"use client";
import { useEffect, useRef } from "react";

type VideoPlayerProps = {
  src: string;
  autoplayOnView?: boolean;
  className?: string;
  children?: React.ReactNode;
};

export const VideoPlayer = ({ src, autoplayOnView, className, children }: VideoPlayerProps) => {
  const ref = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    const v = ref.current;
    if (!v) return;
    const play = () => { void v.play().catch(() => {}); };
    if (autoplayOnView && "IntersectionObserver" in window) {
      const io = new IntersectionObserver(
        (entries) => entries.forEach((e) => (e.isIntersecting ? play() : v.pause())),
        { threshold: 0.3 },
      );
      io.observe(v);
      return () => io.disconnect();
    }
    if (!autoplayOnView) {
      const stage = v.parentElement;
      if (!stage) return;
      const onEnter = () => play();
      const onLeave = () => v.pause();
      stage.addEventListener("mouseenter", onEnter);
      stage.addEventListener("mouseleave", onLeave);
      return () => {
        stage.removeEventListener("mouseenter", onEnter);
        stage.removeEventListener("mouseleave", onLeave);
      };
    }
  }, [autoplayOnView]);

  return (
    <div className="media-stage">
      <video
        ref={ref}
        className={className}
        muted
        loop
        playsInline
        preload="none"
        onLoadedData={(e) => {
          const ph = e.currentTarget.parentElement?.querySelector<HTMLElement>(".media-placeholder");
          if (ph) ph.style.display = "none";
        }}
        onError={(e) => {
          const ph = e.currentTarget.parentElement?.querySelector<HTMLElement>(".media-placeholder");
          if (ph) ph.style.display = "";
        }}
      >
        <source src={src} type="video/mp4" />
      </video>
      {children}
    </div>
  );
};
