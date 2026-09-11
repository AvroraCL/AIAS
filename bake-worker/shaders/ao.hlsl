RaytracingAccelerationStructure scene : register(t0);
struct Surface { float3 position; uint objectId; float3 normal; uint pixel; };
StructuredBuffer<Surface> surfaces : register(t1);
StructuredBuffer<uint> objects : register(t2);
RWStructuredBuffer<uint> hits : register(u0);
cbuffer Params : register(b0) { uint count; uint sampleStart; uint sampleCount; uint totalSamples; float distance; float bias; uint selfOnly; uint reserved; };
uint hash(uint x) { x ^= x >> 16; x *= 0x7feb352d; x ^= x >> 15; x *= 0x846ca68b; return x ^ (x >> 16); }
[numthreads(64,1,1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= count) return;
    Surface s=surfaces[id.x];
    float3 n=normalize(s.normal);
    float3 t=normalize(cross(abs(n.z)<0.999 ? float3(0,0,1):float3(0,1,0),n));
    float3 b=cross(n,t);
    uint blocked=0;
    for(uint k=sampleStart;k<sampleStart+sampleCount;k++) {
        float u=(k+0.5)/totalSamples;
        float v=(hash(s.pixel ^ hash(k+17)) & 0x00ffffff)/16777216.0;
        float r=sqrt(u), angle=6.28318530718*v;
        RayDesc ray;
        ray.Origin=s.position+n*bias; ray.TMin=0; ray.TMax=distance;
        ray.Direction=t*(r*cos(angle))+b*(r*sin(angle))+n*sqrt(1-u);
        RayQuery<RAY_FLAG_FORCE_NON_OPAQUE> query;
        query.TraceRayInline(scene,RAY_FLAG_NONE,255,ray);
        while(query.Proceed()) {
            if(query.CandidateType()==CANDIDATE_NON_OPAQUE_TRIANGLE && (selfOnly==0 || objects[query.CandidatePrimitiveIndex()]==s.objectId)) query.CommitNonOpaqueTriangleHit();
        }
        blocked += query.CommittedStatus()==COMMITTED_TRIANGLE_HIT ? 1:0;
    }
    hits[id.x] = (sampleStart==0 ? 0:hits[id.x]) + blocked;
}
