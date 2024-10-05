## TDS Network Tracer (Developer Tool)

This class logs all TDS traffic to both the console and disk.

## Usage

To use the tracer, follow these steps:

1. Add the `NetworkTracer.cs` file to your  https://github.com/dotnet/SqlClient branch
   
2. Replace the `SetPacketData` and `SNIPacketGetData` methods located at:
   `src\Microsoft.Data.SqlClient\netcore\src\Microsoft\Data\SqlClient\TdsParserStateObjectNative.cs`

   with the following methods:
```
```csharp
static Microsoft.Data.SqlClient.Tracer.NetworkTracer tracer = new Tracer.NetworkTracer();

internal override void SetPacketData(PacketHandle packet, byte[] buffer, int bytesUsed)
{
    Debug.Assert(packet.Type == PacketHandle.NativePacketType, "unexpected packet type when requiring NativePacket");
    tracer.SendDataToNetworkStream(buffer, 0, bytesUsed).GetAwaiter().GetResult();
    SNINativeMethodWrapper.SNIPacketSetData(packet.NativePacket, buffer, bytesUsed);
}

protected override uint SNIPacketGetData(PacketHandle packet, byte[] _inBuff, ref uint dataSize)
{
    Debug.Assert(packet.Type == PacketHandle.NativePointerType, "unexpected packet type when requiring NativePointer");
    var result = SNINativeMethodWrapper.SNIPacketGetData(packet.NativePointer, _inBuff, ref dataSize);

    tracer.GetDataFromNetworkStream(_inBuff, 0, (int)dataSize).GetAwaiter().GetResult();
    return result;
}
```

3. After making these changes, when you run the console application, you will be able to view the packet details in the console, and a file will also be created in the `bin` folder for the session.

Tacer can  support  both Normal and Mars packets.
Note: It wont work if you have more than one connection which are used in parallel.

Future Enhancements
I might develop a Windows app to deserialize each request and response for easier readability. In the meantime, I recommend leveraging ChatGPT for interpreting payloads, as it provides a good translation of each packet.

You can also use this specialized ChatGPT for TDS-specific queries:  
[ChatGPT SQL Server Driver Expert](https://chatgpt.com/g/g-sM5P67E6W-sql-server-driver-expert)
```



This format includes headings, code blocks, and a cleaner structure for readability. You can now copy-paste this directly into your GitHub repository.
