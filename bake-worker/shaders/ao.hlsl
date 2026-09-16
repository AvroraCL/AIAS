RaytracingAccelerationStructure scene : register(t0);
struct Surface { float3 position; uint objectId; float3 normal; uint pixel; };
StructuredBuffer<Surface> surfaces : register(t1);
StructuredBuffer<uint> objects : register(t2);
RWStructuredBuffer<uint> hits : register(u0);
cbuffer Params : register(b0) { uint count; uint sampleStart; uint sampleCount; uint totalSamples; float distance; float bias; uint selfOnly; uint mode; };
uint hash(uint x) { x ^= x >> 16; x *= 0x7feb352d; x ^= x >> 15; x *= 0x846ca68b; return x ^ (x >> 16); }
[numthreads(64,1,1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= count) return;
    Surface s=surfaces[id.x];
    float3 n=normalize(s.normal);
    float3 t=normalize(cross(abs(n.z)<0.999 ? float3(0,0,1):float3(0,1,0),n));
    float3 b=cross(n,t);
    // 方位角：每像素固定随机相位 + 确定性均匀分层（Cranley-Patterson 旋转）。
    // 纯白噪声收敛 O(1/sqrt(N))，分层后近似 O(1/N)，32/64 采样噪点显著下降。
    float azimuthPhase=(hash(s.pixel) & 0x00ffffff)/16777216.0;
    uint value=0;
    for(uint k=sampleStart;k<sampleStart+sampleCount;k++) {
        float u=(k+0.5)/totalSamples;
        float v=frac(azimuthPhase+(k+0.5)/totalSamples);
        float r=sqrt(u), angle=6.28318530718*v;
        RayDesc ray;
        float3 direction=t*(r*cos(angle))+b*(r*sin(angle))+n*sqrt(1-u);
        if (mode == 1) direction = -direction;
        // 防自交交给 TMin（不移动射线起点）：沿插值法线偏移 origin 会在平滑折痕
        // 处把起点挪进邻面之下，产生假遮蔽黑斑，薄壁件厚度也会读零。
        ray.Origin=s.position; ray.TMin=bias; ray.TMax=distance;
        ray.Direction=direction;
        // selfOnly=1：BLAS 为非不透明几何，逐候选做对象过滤后提交。
        if (selfOnly == 1) {
            RayQuery<RAY_FLAG_FORCE_NON_OPAQUE> query;
            query.TraceRayInline(scene,RAY_FLAG_NONE,255,ray);
            while(query.Proceed()) {
                if(query.CandidateType()==CANDIDATE_NON_OPAQUE_TRIANGLE && objects[query.CandidatePrimitiveIndex()]==s.objectId) query.CommitNonOpaqueTriangleHit();
            }
            if(query.CommittedStatus()==COMMITTED_TRIANGLE_HIT) {
                value += mode == 0 ? 1 : (uint)round(saturate(query.CommittedRayT()/distance)*65535.0);
            }
        } else if (mode == 0) {
            // selfOnly=0：几何标记 OPAQUE，命中自动提交；AO 只计二值命中，
            // 任一命中即终止遍历，走硬件最快路径。不透明命中在 Proceed 中
            // 自动提交，首个命中后遍历即结束。
            RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> query;
            query.TraceRayInline(scene,RAY_FLAG_NONE,255,ray);
            while(query.Proceed()) {}
            if(query.CommittedStatus()==COMMITTED_TRIANGLE_HIT) value += 1;
        } else {
            // 厚度：完整遍历取最近命中距离。
            RayQuery<RAY_FLAG_NONE> query;
            query.TraceRayInline(scene,RAY_FLAG_NONE,255,ray);
            while(query.Proceed()) {}
            if(query.CommittedStatus()==COMMITTED_TRIANGLE_HIT) {
                value += (uint)round(saturate(query.CommittedRayT()/distance)*65535.0);
            }
        }
    }
    hits[id.x] = (sampleStart==0 ? 0:hits[id.x]) + value;
}
