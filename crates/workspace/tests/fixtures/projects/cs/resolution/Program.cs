namespace Resolution;

public sealed class Program
{
	private string Format(string value) => value;

    public string Run(Worker worker, object runtime)
    {
        worker.Format("value");
        runtime.MissingRuntimeMember();
        return "done";
    }
}
